// Vendored from https://github.com/askrjs/askr-monaco (@askrjs/monaco is not
// yet published to npm and ships no built dist/, so a git dependency won't
// resolve without extra build tooling). Source commit
// f27410ca1cda918225123bd5f1581dd4cd698346, src/components/monaco-editor/monaco-editor.tsx,
// unmodified except for this note. Replace this vendor folder with a real
// `@askrjs/monaco` dependency once it's published.

import { state } from "@askrjs/askr";
import { task } from "@askrjs/askr/resources";
import { setRef, type Ref } from "@askrjs/askr/foundations/utilities";
import type * as Monaco from "monaco-editor";
import type {
  MonacoBeforeMount,
  MonacoEditorInstance,
  MonacoEditorOptions,
  MonacoEditorProps,
  MonacoErrorHandler,
  MonacoLoader,
  MonacoMountHandler,
  MonacoNamespace,
  MonacoTextModel,
  MonacoUri,
} from "./monaco-editor.types";

type MonacoController = {
  host: HTMLDivElement | null;
  editor: MonacoEditorInstance | null;
  monaco: MonacoNamespace | null;
  ownedModel: MonacoTextModel | null;
  ownedModelPath: string | null;
  currentProps: MonacoEditorProps;
  ref: (node: HTMLDivElement | null) => void;
  scheduled: boolean;
  disposed: boolean;
  applyGeneration: number;
  loadGeneration: number;
  loadingPromise: Promise<void> | null;
  loadingLoader: MonacoLoader;
  monacoSource: "provided" | "loaded" | null;
  lastHostRef: Ref<HTMLDivElement>;
  lastEditorRef: Ref<MonacoEditorInstance>;
  lastMonacoRef: Ref<MonacoNamespace>;
  lastOverrideServices: MonacoEditorProps["overrideServices"] | undefined;
  lastOverflowWidgetsDomNode: HTMLElement | undefined;
  lastAriaContainerElement: HTMLElement | undefined;
  lastOptions: MonacoEditorOptions | undefined;
  lastTheme: string | undefined;
};

function defaultLoadMonaco() {
  return import("monaco-editor");
}

function createController(initialProps: MonacoEditorProps): MonacoController {
  const controller: MonacoController = {
    host: null,
    editor: null,
    monaco: null,
    ownedModel: null,
    ownedModelPath: null,
    currentProps: initialProps,
    ref: (node) => {
      controller.host = node;

      if (node === null) {
        controller.applyGeneration += 1;
        disposeEditor(controller, true);
        syncHostRef(controller);
        syncEditorRef(controller);
        syncMonacoRef(controller);
        return;
      }

      scheduleApply(controller);
    },
    scheduled: false,
    disposed: false,
    applyGeneration: 0,
    loadGeneration: 0,
    loadingPromise: null,
    loadingLoader: undefined,
    monacoSource: null,
    lastHostRef: undefined,
    lastEditorRef: undefined,
    lastMonacoRef: undefined,
    lastOverrideServices: undefined,
    lastOverflowWidgetsDomNode: undefined,
    lastAriaContainerElement: undefined,
    lastOptions: undefined,
    lastTheme: undefined,
  };

  return controller;
}

function scheduleApply(controller: MonacoController) {
  if (controller.disposed || controller.host === null || typeof queueMicrotask !== "function") {
    return;
  }

  if (controller.scheduled) {
    return;
  }

  controller.scheduled = true;
  queueMicrotask(() => {
    controller.scheduled = false;
    void applyController(controller);
  });
}

function syncHostRef(controller: MonacoController) {
  const nextRef = controller.currentProps.ref;

  if (controller.lastHostRef !== nextRef) {
    setRef(controller.lastHostRef, null);
    controller.lastHostRef = nextRef;
  }

  setRef(controller.lastHostRef, controller.host);
}

function syncEditorRef(controller: MonacoController) {
  const nextRef = controller.currentProps.editorRef;

  if (controller.lastEditorRef !== nextRef) {
    setRef(controller.lastEditorRef, null);
    controller.lastEditorRef = nextRef;
  }

  setRef(controller.lastEditorRef, controller.editor);
}

function syncMonacoRef(controller: MonacoController) {
  const nextRef = controller.currentProps.monacoRef;

  if (controller.lastMonacoRef !== nextRef) {
    setRef(controller.lastMonacoRef, null);
    controller.lastMonacoRef = nextRef;
  }

  setRef(controller.lastMonacoRef, controller.monaco);
}

function normalizePath(path: MonacoEditorProps["path"]): string | null {
  if (typeof path === "string") {
    return path;
  }

  return path ? path.toString() : null;
}

function resolveUri(
  monaco: MonacoNamespace,
  path: MonacoEditorProps["path"],
): MonacoUri | undefined {
  if (typeof path === "string") {
    return monaco.Uri.parse(path);
  }

  return path ?? undefined;
}

function assertOwnedPathAvailable(
  controller: MonacoController,
  monaco: MonacoNamespace,
  uri: MonacoUri | undefined,
) {
  if (!uri) {
    return;
  }

  const existingModel = monaco.editor.getModel(uri);

  if (existingModel && existingModel !== controller.ownedModel) {
    throw new Error(
      `@askrjs/monaco: path "${uri.toString()}" already has a Monaco model. ` +
        "Pass that model through the model prop when sharing is intentional.",
    );
  }
}

function createOwnedModel(controller: MonacoController, monaco: MonacoNamespace, value: string) {
  const uri = resolveUri(monaco, controller.currentProps.path);
  assertOwnedPathAvailable(controller, monaco, uri);
  const model = monaco.editor.createModel(value, controller.currentProps.language, uri);

  controller.ownedModel = model;
  controller.ownedModelPath = normalizePath(controller.currentProps.path);
  return model;
}

function disposeOwnedModel(controller: MonacoController) {
  controller.ownedModel?.dispose();
  controller.ownedModel = null;
  controller.ownedModelPath = null;
}

function recreateOwnedModel(controller: MonacoController, monaco: MonacoNamespace, value: string) {
  const uri = resolveUri(monaco, controller.currentProps.path);
  assertOwnedPathAvailable(controller, monaco, uri);
  disposeOwnedModel(controller);
  return createOwnedModel(controller, monaco, value);
}

function resolveModel(
  controller: MonacoController,
  monaco: MonacoNamespace,
): MonacoTextModel | null {
  const { model, value, defaultValue, language, path } = controller.currentProps;

  if (model !== undefined) {
    disposeOwnedModel(controller);
    return model;
  }

  if (controller.ownedModel === null) {
    return createOwnedModel(controller, monaco, value ?? defaultValue ?? "");
  }

  const nextPath = normalizePath(path);
  const controlledValue = value;

  if (controller.ownedModelPath !== nextPath) {
    const nextValue = controlledValue ?? controller.ownedModel.getValue();
    return recreateOwnedModel(controller, monaco, nextValue);
  }

  if (language && controller.ownedModel.getLanguageId() !== language) {
    monaco.editor.setModelLanguage(controller.ownedModel, language);
  }

  if (controlledValue !== undefined && controller.ownedModel.getValue() !== controlledValue) {
    controller.ownedModel.setValue(controlledValue);
  }

  return controller.ownedModel;
}

function getCreateOnlyNodes(options: MonacoEditorOptions | undefined) {
  return {
    overflowWidgetsDomNode: options?.overflowWidgetsDomNode,
    ariaContainerElement: options?.ariaContainerElement,
  };
}

function buildCreateOptions(
  controller: MonacoController,
  model: MonacoTextModel | null,
): Monaco.editor.IStandaloneEditorConstructionOptions {
  return {
    ...controller.currentProps.options,
    model,
  };
}

function buildUpdateOptions(
  options: MonacoEditorOptions | undefined,
): Monaco.editor.IEditorOptions & Monaco.editor.IGlobalEditorOptions {
  if (!options) {
    return {};
  }

  const {
    ariaContainerElement: _ariaContainerElement,
    dimension: _dimension,
    overflowWidgetsDomNode: _overflowWidgetsDomNode,
    ...rest
  } = options;

  return rest;
}

function notifyError(handler: MonacoErrorHandler, error: unknown) {
  if (handler) {
    handler(error);
    return;
  }

  void Promise.resolve().then(() => {
    throw error;
  });
}

async function runBeforeMount(beforeMount: MonacoBeforeMount, monaco: MonacoNamespace) {
  if (!beforeMount) {
    return;
  }

  await beforeMount(monaco);
}

function runMountHandler(handler: MonacoMountHandler, controller: MonacoController) {
  if (!handler || controller.editor === null || controller.monaco === null) {
    return;
  }

  handler(controller.editor, controller.monaco);
}

function isApplyCurrent(controller: MonacoController, monaco: MonacoNamespace, generation: number) {
  return (
    !controller.disposed &&
    controller.host !== null &&
    controller.monaco === monaco &&
    controller.applyGeneration === generation
  );
}

function disposeEditor(
  controller: MonacoController,
  clearMonaco: boolean,
  preserveOwnedModel = false,
) {
  if (controller.editor !== null) {
    runMountHandler(controller.currentProps.onUnmount, controller);
    controller.editor.dispose();
    controller.editor = null;
  }

  syncEditorRef(controller);
  if (!preserveOwnedModel) {
    disposeOwnedModel(controller);
  }
  controller.lastOverrideServices = undefined;
  controller.lastOverflowWidgetsDomNode = undefined;
  controller.lastAriaContainerElement = undefined;
  controller.lastOptions = undefined;
  controller.lastTheme = undefined;

  if (clearMonaco) {
    controller.monaco = null;
    controller.monacoSource = null;
    syncMonacoRef(controller);
  }
}

function needsEditorRecreate(controller: MonacoController) {
  const { options, overrideServices } = controller.currentProps;
  const { overflowWidgetsDomNode, ariaContainerElement } = getCreateOnlyNodes(options);

  return (
    controller.editor === null ||
    controller.lastOverrideServices !== overrideServices ||
    controller.lastOverflowWidgetsDomNode !== overflowWidgetsDomNode ||
    controller.lastAriaContainerElement !== ariaContainerElement
  );
}

async function ensureEditor(
  controller: MonacoController,
  monaco: MonacoNamespace,
  generation: number,
) {
  const shouldRecreate = needsEditorRecreate(controller);

  if (!shouldRecreate && controller.editor !== null) {
    const model = resolveModel(controller, monaco);

    if (controller.editor.getModel() !== model) {
      controller.editor.setModel(model);
    }

    return;
  }

  disposeEditor(controller, false, true);

  await runBeforeMount(controller.currentProps.beforeMount, monaco);

  if (!isApplyCurrent(controller, monaco, generation)) {
    return;
  }

  const model = resolveModel(controller, monaco);

  if (controller.currentProps.theme) {
    monaco.editor.setTheme(controller.currentProps.theme);
  }

  const nextEditor = monaco.editor.create(
    controller.host as HTMLDivElement,
    buildCreateOptions(controller, model),
    controller.currentProps.overrideServices,
  );

  controller.editor = nextEditor;
  controller.lastOverrideServices = controller.currentProps.overrideServices;
  controller.lastOverflowWidgetsDomNode = controller.currentProps.options?.overflowWidgetsDomNode;
  controller.lastAriaContainerElement = controller.currentProps.options?.ariaContainerElement;
  controller.lastOptions = controller.currentProps.options;
  controller.lastTheme = controller.currentProps.theme;
  syncEditorRef(controller);
  runMountHandler(controller.currentProps.onMount, controller);
}

function applyEditorUpdates(controller: MonacoController, monaco: MonacoNamespace) {
  if (controller.editor === null) {
    return;
  }

  const { options, theme } = controller.currentProps;

  if (controller.lastOptions !== options) {
    controller.editor.updateOptions(buildUpdateOptions(options));
    controller.lastOptions = options;
  }

  if (options?.dimension) {
    controller.editor.layout(options.dimension);
  }

  if (theme && controller.lastTheme !== theme) {
    monaco.editor.setTheme(theme);
    controller.lastTheme = theme;
  }
}

function startLoadingMonaco(controller: MonacoController, loader: MonacoLoader) {
  const load = loader ?? defaultLoadMonaco;

  if (controller.loadingPromise && controller.loadingLoader === load) {
    return;
  }

  const generation = ++controller.loadGeneration;
  controller.loadingLoader = load;
  controller.loadingPromise = Promise.resolve(load()).then(
    (monaco) => {
      controller.loadingPromise = null;

      if (
        controller.disposed ||
        controller.loadGeneration !== generation ||
        controller.currentProps.monaco !== undefined
      ) {
        return;
      }

      controller.monaco = monaco;
      controller.monacoSource = "loaded";
      syncMonacoRef(controller);
      scheduleApply(controller);
    },
    (error) => {
      controller.loadingPromise = null;

      if (!controller.disposed && controller.loadGeneration === generation) {
        notifyError(controller.currentProps.onError, error);
      }
    },
  );
}

async function applyController(controller: MonacoController) {
  if (controller.disposed || controller.host === null) {
    return;
  }

  const generation = ++controller.applyGeneration;

  syncHostRef(controller);

  if (controller.currentProps.monaco) {
    if (
      controller.monaco !== controller.currentProps.monaco ||
      controller.monacoSource !== "provided"
    ) {
      controller.loadGeneration += 1;
      controller.loadingPromise = null;
      disposeEditor(controller, true);
      controller.monaco = controller.currentProps.monaco;
      controller.monacoSource = "provided";
      syncMonacoRef(controller);
    }
  } else if (controller.monacoSource === "provided") {
    disposeEditor(controller, true);
    startLoadingMonaco(controller, controller.currentProps.loadMonaco);
    return;
  } else if (controller.monaco === null) {
    startLoadingMonaco(controller, controller.currentProps.loadMonaco);
    return;
  }

  const monaco = controller.monaco;

  if (monaco === null) {
    return;
  }

  try {
    await ensureEditor(controller, monaco, generation);

    if (!isApplyCurrent(controller, monaco, generation)) {
      return;
    }

    applyEditorUpdates(controller, monaco);
  } catch (error) {
    if (isApplyCurrent(controller, monaco, generation)) {
      notifyError(controller.currentProps.onError, error);
    }
  }
}

/**
 * Monaco wrapper that keeps the Askr surface thin and forwards real Monaco
 * models, options, and lifecycle access instead of recreating Monaco's API.
 */
export function MonacoEditor(props: MonacoEditorProps): JSX.Element {
  // Upstream calls state(...) as if it returned a single callable getter;
  // the installed @askrjs/askr (0.0.51) returns a [getter, setter] tuple
  // instead (matches every other state() call in this codebase), so this
  // line is adapted from the vendored source to destructure it. The setter
  // is intentionally unused — this object is mutated in place across
  // renders, not replaced, so state() is only being used to persist a
  // stable reference (equivalent to a ref), not to trigger re-renders.
  const [getController] = state(createController(props));
  const controller = getController();
  controller.currentProps = props;
  controller.disposed = false;

  task(() => {
    return () => {
      controller.disposed = true;
      controller.host = null;
      controller.applyGeneration += 1;
      controller.loadGeneration += 1;
      controller.loadingPromise = null;
      disposeEditor(controller, true);
      syncHostRef(controller);
      syncEditorRef(controller);
      syncMonacoRef(controller);
    };
  });

  scheduleApply(controller);

  const hasAccessibleLabel =
    typeof props["aria-label"] === "string" || props["aria-labelledby"] != null;

  const {
    beforeMount: _beforeMount,
    defaultValue: _defaultValue,
    editorRef: _editorRef,
    language: _language,
    loadMonaco: _loadMonaco,
    model: _model,
    monaco: _monaco,
    monacoRef: _monacoRef,
    onError: _onError,
    onMount: _onMount,
    onUnmount: _onUnmount,
    options: _options,
    overrideServices: _overrideServices,
    path: _path,
    ref: _ref,
    theme: _theme,
    value: _value,
    ...hostProps
  } = props;

  return (
    <div
      {...hostProps}
      ref={controller.ref}
      role={props.role ?? (hasAccessibleLabel ? "region" : undefined)}
      data-askr-monaco-editor
    />
  );
}
