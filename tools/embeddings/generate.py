#!/usr/bin/env python3
"""Generate the #1365 embedding workload files (workstation only; never run in CI).

Reads the committed manifest (tests/corpus/embeddings/manifest.json) for the corpus
revision, models, preprocessing and set sizes, downloads the BEIR CQADupStack parquet
files into target/embeddings/cache, selects documents deterministically, embeds them with
the pinned model revisions, L2-normalizes, writes little-endian float32 rows plus id files
under target/embeddings/, prints the SHA-256 of every output, and with --update-manifest
writes the query split and the file checksums back into the manifest.

No corpus text or vectors are committed; see tools/embeddings/README.md.
"""
import argparse
import hashlib
import json
import re
import sys
import time
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "target" / "embeddings"
MANIFEST = REPO / "tests" / "corpus" / "embeddings" / "manifest.json"


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def sort_key(prefix, doc_id):
    """Selection/split ordering key: sha256(prefix + ':' + composite id), ties by id."""
    return (hashlib.sha256(f"{prefix}:{doc_id}".encode()).hexdigest(), doc_id)


def normalize_text(value):
    return " ".join((value or "").split())


def preprocess(title, body):
    """title + newline + body, each whitespace-normalized; title omitted when empty."""
    title, body = normalize_text(title), normalize_text(body)
    return f"{title}\n{body}" if title and body else title or body


def download(manifest, kind, subforum):
    from huggingface_hub import hf_hub_download

    source = manifest["corpus"]["source"]
    return hf_hub_download(
        repo_id=source["beir_dataset_id"],
        repo_type="dataset",
        revision=source["beir_dataset_revision"],
        filename=f"{subforum}/{kind}/{kind}-00000-of-00001.parquet",
        cache_dir=str(OUT / "cache" / "hf"),
    )


def load_table(manifest, kind, subforum, columns):
    table = pq.read_table(download(manifest, kind, subforum), columns=columns)
    return {name: table.column(name).to_pylist() for name in columns}


def compose(subforum, raw_id):
    return f"{subforum}/{raw_id}"


def load_queries(manifest):
    queries = {}
    for subforum in manifest["corpus"]["subforums"]:
        cols = load_table(manifest, "queries", subforum, ["_id", "title", "text"])
        for raw, title, text in zip(cols["_id"], cols["title"], cols["text"]):
            queries[compose(subforum, raw)] = preprocess(title, text)
    return queries


def select_documents(manifest, query_ids, largest):
    """Hash-ordered first-N documents, excluding every BEIR query document."""
    ranked = []
    for subforum in manifest["corpus"]["subforums"]:
        ids = load_table(manifest, "corpus", subforum, ["_id"])["_id"]
        for raw in ids:
            doc_id = compose(subforum, raw)
            if doc_id not in query_ids:
                ranked.append(sort_key("select", doc_id))
    ranked.sort()
    if len(ranked) < largest:
        sys.exit(f"corpus has only {len(ranked)} eligible documents, need {largest}")
    return [doc_id for _, doc_id in ranked[:largest]]


def document_texts(manifest, wanted):
    wanted_set = set(wanted)
    texts = {}
    for subforum in manifest["corpus"]["subforums"]:
        cols = load_table(manifest, "corpus", subforum, ["_id", "title", "text"])
        for raw, title, text in zip(cols["_id"], cols["title"], cols["text"]):
            doc_id = compose(subforum, raw)
            if doc_id in wanted_set:
                texts[doc_id] = preprocess(title, text)
    return [texts[doc_id] for doc_id in wanted]


def split_queries(manifest, query_ids):
    spec = manifest["queries"]
    ordered = [q for _, q in sorted(sort_key("split", q) for q in query_ids)]
    tuning = ordered[: spec["tuning"]["count"]]
    end = spec["tuning"]["count"] + spec["held_out"]["count"]
    return tuning, ordered[spec["tuning"]["count"] : end]


def embed(model, texts, chunk_size, batch_size):
    """Fixed-size chunks keep every chunk independent of the total, so a smaller set is
    a bit-exact prefix of a larger one on the same machine and library versions."""
    parts = []
    started = time.time()
    for start in range(0, len(texts), chunk_size):
        chunk = texts[start : start + chunk_size]
        vectors = model.encode(
            chunk,
            batch_size=batch_size,
            normalize_embeddings=False,
            convert_to_numpy=True,
            show_progress_bar=False,
        ).astype(np.float64)
        vectors /= np.maximum(np.linalg.norm(vectors, axis=1, keepdims=True), 1e-12)
        parts.append(vectors.astype("<f4"))
        done = start + len(chunk)
        print(f"  embedded {done}/{len(texts)} ({time.time() - started:.0f}s)", flush=True)
    return np.concatenate(parts)


def write_rows(path, array):
    path.write_bytes(np.ascontiguousarray(array, dtype="<f4").tobytes())


def write_lines(path, items):
    path.write_text("".join(f"{item}\n" for item in items), encoding="utf-8")


def collapse_string_arrays(text):
    pattern = re.compile(r'\[\n\s+"[^\n]*"(?:,\n\s+"[^\n]*")*\n\s+\]')
    return pattern.sub(lambda m: "[" + ", ".join(re.findall(r'"[^"\n]*"', m.group(0))) + "]", text)


def save_manifest(manifest):
    MANIFEST.write_text(collapse_string_arrays(json.dumps(manifest, indent=2)) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--models", default="bge-small,bge-base", help="comma list of manifest model keys")
    parser.add_argument(
        "--sets",
        default="10k",
        help="comma list of manifest set names; the largest is embedded and smaller sets are prefixes",
    )
    parser.add_argument("--update-manifest", action="store_true", help="write query split and checksums into the manifest")
    parser.add_argument("--device", default="cpu")
    args = parser.parse_args()

    manifest = json.loads(MANIFEST.read_text())
    OUT.mkdir(parents=True, exist_ok=True)
    gen = manifest["generation"]
    sets = {s["name"]: s["documents"] for s in manifest["sets"]}
    wanted_sets = sorted(args.sets.split(","), key=lambda name: sets[name])
    largest = sets[wanted_sets[-1]]
    corpus = manifest["corpus"]["name"]

    print("loading queries and selecting documents", flush=True)
    queries = load_queries(manifest)
    tuning, held_out = split_queries(manifest, set(queries))
    query_order = tuning + held_out
    doc_ids = select_documents(manifest, set(queries), largest)
    doc_texts = document_texts(manifest, doc_ids)
    query_texts = [queries[q] for q in query_order]

    produced = {}

    def record(path, rows, dims):
        sha = sha256_file(path)
        info = {"sha256": sha, "bytes": path.stat().st_size, "rows": rows}
        if dims is not None:
            info["dimensions"] = dims
        produced[path.name] = info
        print(f"sha256 {sha}  {path.relative_to(REPO)}", flush=True)

    for name in wanted_sets:
        path = OUT / f"{corpus}-{name}.ids.txt"
        write_lines(path, doc_ids[: sets[name]])
        record(path, sets[name], None)
    path = OUT / f"{corpus}-queries.ids.txt"
    write_lines(path, query_order)
    record(path, len(query_order), None)

    from sentence_transformers import SentenceTransformer

    for key in args.models.split(","):
        spec = next(m for m in manifest["models"] if m["key"] == key)
        started = time.time()
        model = SentenceTransformer(
            spec["id"],
            revision=spec["revision"],
            device=args.device,
            cache_folder=str(OUT / "cache" / "models"),
        )
        model.max_seq_length = manifest["preprocessing"]["max_seq_length_tokens"]
        dims = model.get_embedding_dimension()
        assert dims == spec["dimensions"], (key, dims)
        print(f"embedding {len(doc_texts)} documents with {key}", flush=True)
        docs = embed(model, doc_texts, gen["chunk_size"], gen["batch_size"])
        for name in wanted_sets:
            path = OUT / f"{corpus}-{spec['file_stem']}-{name}.f32"
            write_rows(path, docs[: sets[name]])
            record(path, sets[name], dims)
        print(f"embedding {len(query_texts)} queries with {key}", flush=True)
        qvecs = embed(model, query_texts, gen["chunk_size"], gen["batch_size"])
        path = OUT / f"{corpus}-{spec['file_stem']}-queries.f32"
        write_rows(path, qvecs)
        record(path, len(query_texts), dims)
        print(f"model {key} done in {time.time() - started:.0f}s", flush=True)

    if args.update_manifest:
        manifest["queries"]["tuning"]["ids"] = tuning
        manifest["queries"]["held_out"]["ids"] = held_out
        files = {f["name"]: f for f in manifest["files"]}
        for name, info in produced.items():
            files[name] = {"name": name, **info}
        manifest["files"] = sorted(files.values(), key=lambda f: f["name"])
        save_manifest(manifest)
        print(f"updated {MANIFEST.relative_to(REPO)}")


if __name__ == "__main__":
    main()
