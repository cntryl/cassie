from pathlib import Path
import html,json,re,subprocess,urllib.parse
root=Path.cwd();baseline='c23a27604e29bea563bb2ee9f85009514d43f666'
changed=set()
for command in [['git','diff','--name-only',baseline],['git','ls-files','--others','--exclude-standard']]:
    changed.update(p for p in subprocess.check_output(command,text=True).splitlines() if p.endswith('.md'))
changed=sorted(changed)
inline=re.compile(r'\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+[^)]*)?\)')
reference=re.compile(r'^\s*\[[^\]]+\]:\s*(\S+)',re.M)
def strip_fences(text):
    output=[];fence=False
    for line in text.splitlines():
        if line.lstrip().startswith(('```','~~~')): fence=not fence;output.append('');continue
        output.append('' if fence else line)
    return '\n'.join(output)
def links(text):
    text=strip_fences(text)
    return [match.group(1).strip('<>') for pattern in [inline,reference] for match in pattern.finditer(text)]
def anchors(text):
    result=set(re.findall(r'<a\s+[^>]*(?:id|name)=[\"\']([^\"\']+)',text,re.I));seen={}
    for line in strip_fences(text).splitlines():
        match=re.match(r'^ {0,3}#{1,6}\s+(.*?)\s*#*\s*$',line)
        if not match: continue
        title=html.unescape(re.sub(r'<[^>]+>','',match.group(1))).lower()
        slug=re.sub(r'[^\w\-\s]','',title)
        slug=re.sub(r'\s','-',slug)
        index=seen.get(slug,0);seen[slug]=index+1
        result.add(slug if index==0 else f'{slug}-{index}')
    return result
def source(path,old):
    if old:
        p=subprocess.run(['git','show',f'{baseline}:{path}'],text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        return None if p.returncode else p.stdout
    p=root/path
    return p.read_text(errors='replace') if p.is_file() else None
def check(path,target,old=False):
    url=urllib.parse.urlsplit(target)
    if url.scheme or url.netloc: return None
    decoded=urllib.parse.unquote(url.path)
    if decoded.startswith('/'): return None
    destination=(root/path).parent/decoded if decoded else root/path
    try: rel=destination.resolve().relative_to(root).as_posix()
    except ValueError: return None
    text=source(rel,old)
    if text is None:
        if not old and destination.is_dir(): return None
        return {'file':path,'target':target,'destination':rel,'reason':'missing local file'}
    if url.fragment and destination.suffix.lower()=='.md':
        anchor=urllib.parse.unquote(url.fragment)
        if anchor not in anchors(text): return {'file':path,'target':target,'destination':rel,'reason':'missing Markdown anchor','available_anchors':sorted(anchors(text))}
    return None
failures=[];introduced=[];tested=0
for path in changed:
    text=source(path,False)
    if text is None: continue
    old_targets=set(links(source(path,True) or ''))
    for target in links(text):
        tested+=1
        issue=check(path,target)
        if issue:
            failures.append(issue)
            if target not in old_targets or check(path,target,True) is None: introduced.append(issue)
legacy=[]
for path in [root/'README.md',*sorted((root/'docs').rglob('*.md'))]:
    for target in links(path.read_text(errors='replace')):
        if 'experimental-promotion-criteria.md' in target or 'feature-qualification.md' in target:
            rel=path.relative_to(root).as_posix();issue=check(rel,target)
            legacy.append({'file':rel,'target':target,'failure':issue})
report={'scope':'Current worktree including uncommitted final documentation; source is Git parent identity, not a committed-document assertion.','working_tree_status':subprocess.check_output(['git','status','--porcelain'],text=True),'source':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'baseline':baseline,'changed_markdown_files':len(changed),'changed_doc_links_inspected':tested,'current_failures':failures,'newly_broken_compared_with_baseline':introduced,'policy_redirect_and_new_file_callers':legacy,'probe_limit':'Inline/reference local links and ATX headings/explicit anchors only; external URLs skipped. Suspected slug differences require manual confirmation.'}
(root/'target/assembly-line/replay-qualification/doc-link-probe.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({key:report[key] for key in ['source','changed_markdown_files','changed_doc_links_inspected','newly_broken_compared_with_baseline']},indent=2))
print('policy caller failures',sum(item['failure'] is not None for item in legacy),'of',len(legacy))

raise SystemExit(1 if introduced else 0)
