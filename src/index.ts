/**
 * @actualwave/react-native-swc — on-device SWC (Rust) for React Native, Android only.
 *
 * Every function has an async form and a `…Sync` form (which blocks the JS thread). Native errors
 * (syntax errors, invalid options, an SWC panic) surface as thrown `Error`s whose message is
 * `file:line:col: message` for located syntax errors.
 */
import { NativeModules, Platform } from 'react-native';

/** SWC `Options` subset (https://swc.rs/docs/configuration/compilation) — any other key passes through. */
export interface SwcOptions {
  env?: { targets?: string | Record<string, string>; include?: string[]; exclude?: string[]; [key: string]: unknown };
  jsc?: {
    parser?: Record<string, unknown>;
    target?: string;
    transform?: Record<string, unknown>;
    externalHelpers?: boolean;
    keepClassNames?: boolean;
    loose?: boolean;
    [key: string]: unknown;
  };
  module?: { type: string; [key: string]: unknown };
  sourceMaps?: boolean | 'inline';
  inputSourceMap?: boolean | string;
  [key: string]: unknown;
}

export interface Dependency {
  /** The module specifier exactly as written (`'react'`, `'./util'`). */
  specifier: string;
  /**
   * `import` — `import … from`; `export-from` — `export {…} from`; `export-all` — `export * from`;
   * `dynamic` — `import('x')`; `require` — `require('x')`. Only string-literal specifiers are listed.
   */
  kind: 'import' | 'export-from' | 'export-all' | 'dynamic' | 'require';
  /** Bound names: `default`, `*` (namespace) and named imports / re-exports. Omitted when empty. */
  names?: string[];
}

export interface Analysis {
  imports: Dependency[];
  /** Exported binding names, `default` included. `export * from` is only listed in `imports`. */
  exports: string[];
}

export interface TransformResult extends Analysis {
  code: string;
  /** Source map v3 JSON string, present when `sourceMaps` is enabled (default). */
  map?: string;
}

/** Plain minify output — no import/export analysis. */
export interface MinifyResult {
  code: string;
  map?: string;
}

/** ESTree-like SWC AST (`Module` or `Script`); shape follows `swc_ecma_ast` serialization. */
export interface Program {
  type: 'Module' | 'Script';
  body: unknown[];
  span: { start: number; end: number; ctxt: number };
  [key: string]: unknown;
}

export interface PrintOptions {
  /** ES version string such as `es2022`; defaults to the latest. */
  target?: string;
  minify?: boolean;
  asciiOnly?: boolean;
}

/**
 * SWC `JsMinifyOptions` subset (https://swc.rs/docs/configuration/minification) — any other key
 * passes through. Defaults: `compress` and `mangle` on, source maps off.
 */
export interface MinifyOptions {
  compress?: boolean | Record<string, unknown>;
  mangle?: boolean | Record<string, unknown>;
  format?: Record<string, unknown>;
  ecma?: number | string;
  module?: boolean;
  toplevel?: boolean;
  sourceMap?: boolean | Record<string, unknown>;
  inlineSourcesContent?: boolean;
  [key: string]: unknown;
}

interface NativeSwc {
  transform(source: string, filename: string, optionsJson: string): Promise<string>;
  transformSync(source: string, filename: string, optionsJson: string): string;
  minify(source: string, filename: string, optionsJson: string): Promise<string>;
  minifySync(source: string, filename: string, optionsJson: string): string;
  parse(source: string, filename: string, optionsJson: string): Promise<string>;
  parseSync(source: string, filename: string, optionsJson: string): string;
  analyze(source: string, filename: string, optionsJson: string): Promise<string>;
  analyzeSync(source: string, filename: string, optionsJson: string): string;
  print(astJson: string, optionsJson: string): Promise<string>;
  printSync(astJson: string, optionsJson: string): string;
}

const native: NativeSwc | undefined = (NativeModules as { ActualwaveSwc?: NativeSwc }).ActualwaveSwc;

/**
 * False on platforms/builds where the native module is not linked (iOS, Expo Go, web). Note that on
 * Android a build that links the module but lacks the `.so` for the device ABI fails at launch
 * instead — see README "Supported ABIs".
 */
export function isAvailable(): boolean {
  return Platform.OS === 'android' && !!native;
}

/**
 * Defaults tuned for running user code on Hermes: TS + JSX (automatic runtime), CommonJS modules,
 * lowering of only what Hermes' compiler rejects (classes, async functions/arrows, async generators,
 * `for await`, class fields/private members/static blocks — via SWC `env.include`), everything else stays
 * native; inline helpers, source maps. `jsc.target` can't be combined with `env`, so it isn't set.
 */
export function defaultOptions(filename: string): SwcOptions {
  const ts = /\.(ts|tsx|mts|cts)$/i.test(filename);
  const jsx = !/\.(ts|mts|cts)$/i.test(filename);
  return {
    jsc: {
      parser: ts
        ? { syntax: 'typescript', tsx: jsx, decorators: false }
        : { syntax: 'ecmascript', jsx: true },
      transform: { react: { runtime: 'automatic' } },
      externalHelpers: false,
      keepClassNames: true,
    },
    env: {
      targets: 'chrome 110',
      include: [
        'transform-classes',
        'transform-async-to-generator',
        'transform-async-generator-functions',
        'transform-class-properties',
        'transform-private-methods',
        'transform-private-property-in-object',
        'transform-class-static-block',
      ],
    },
    module: { type: 'commonjs' },
    sourceMaps: true,
    isModule: true,
    swcrc: false,
    configFile: false,
  };
}

function merge<T extends Record<string, any>>(a: T, b?: Record<string, any>): T {
  if (!b) return a;
  const out: Record<string, any> = { ...a };
  for (const key of Object.keys(b)) {
    const v = b[key];
    out[key] =
      v && typeof v === 'object' && !Array.isArray(v) && typeof out[key] === 'object'
        ? merge(out[key], v)
        : v;
  }
  return out as T;
}

function requireNative(): NativeSwc {
  if (!native) {
    throw new Error("'@actualwave/react-native-swc' native module is not linked (Android build required).");
  }
  return native;
}

function unwrap<T>(raw: string): T {
  const res = JSON.parse(raw) as T & { error?: string };
  if (res.error !== undefined) throw new Error(res.error);
  return res;
}

function parseResult(raw: string): TransformResult {
  const res = unwrap<Partial<TransformResult> & { map?: string | null }>(raw);
  return {
    code: res.code ?? '',
    map: res.map ?? undefined,
    imports: res.imports ?? [],
    exports: res.exports ?? [],
  };
}

function minifyResult(raw: string): MinifyResult {
  const res = unwrap<{ code?: string; map?: string | null }>(raw);
  return { code: res.code ?? '', map: res.map ?? undefined };
}

function printArgs(ast: Program, options?: PrintOptions): [string, string] {
  return [JSON.stringify(ast), JSON.stringify(options ?? {})];
}

function transformArgs(source: string, filename: string, options?: SwcOptions): [string, string, string] {
  return [source, filename, JSON.stringify(merge(defaultOptions(filename), options))];
}

function minifyArgs(source: string, filename: string, options?: MinifyOptions): [string, string, string] {
  return [source, filename, JSON.stringify({ compress: true, mangle: true, sourceMap: false, ...options })];
}

/**
 * Transforms `source` with SWC. `options` is deep-merged over `defaultOptions(filename)`.
 * Rejects with a message of the form `file:line:col: error` on syntax errors.
 */
export async function transform(
  source: string,
  filename = 'input.jsx',
  options?: SwcOptions,
): Promise<TransformResult> {
  return parseResult(await requireNative().transform(...transformArgs(source, filename, options)));
}

/**
 * Synchronous `transform` — blocks the JS thread until done, so keep it for small inputs.
 * Throws on syntax errors.
 */
export function transformSync(source: string, filename = 'input.jsx', options?: SwcOptions): TransformResult {
  return parseResult(requireNative().transformSync(...transformArgs(source, filename, options)));
}

/** Minifies JavaScript with SWC (compress + mangle by default). Rejects on syntax errors. */
export async function minify(
  source: string,
  filename = 'input.js',
  options?: MinifyOptions,
): Promise<MinifyResult> {
  return minifyResult(await requireNative().minify(...minifyArgs(source, filename, options)));
}

/** Synchronous `minify` — blocks the JS thread until done. Throws on syntax errors. */
export function minifySync(source: string, filename = 'input.js', options?: MinifyOptions): MinifyResult {
  return minifyResult(requireNative().minifySync(...minifyArgs(source, filename, options)));
}

/**
 * Parses `source` into an SWC AST without running any transform. Parser settings come from
 * `defaultOptions(filename)` (override via `options.jsc.parser` / `jsc.target` / `isModule`).
 * Comments are not part of the AST. Large trees are expensive to move across the bridge.
 */
export async function parse(source: string, filename = 'input.jsx', options?: SwcOptions): Promise<Program> {
  return unwrap<{ ast: Program }>(await requireNative().parse(...transformArgs(source, filename, options))).ast;
}

/** Synchronous `parse` — blocks the JS thread. */
export function parseSync(source: string, filename = 'input.jsx', options?: SwcOptions): Program {
  return unwrap<{ ast: Program }>(requireNative().parseSync(...transformArgs(source, filename, options))).ast;
}

/**
 * Prints an AST (from `parse`, optionally edited) back to code. Comments and source maps are not
 * preserved — the AST carries neither — and parentheses are re-inserted where needed.
 */
export async function print(ast: Program, options?: PrintOptions): Promise<{ code: string }> {
  return unwrap<{ code: string }>(await requireNative().print(...printArgs(ast, options)));
}

/** Synchronous `print` — blocks the JS thread. */
export function printSync(ast: Program, options?: PrintOptions): { code: string } {
  return unwrap<{ code: string }>(requireNative().printSync(...printArgs(ast, options)));
}

/**
 * Lists a module's imports / re-exports / `require` / `import()` calls and its export names
 * without transforming it. TypeScript is not stripped here, so value imports that are only used
 * as types are still listed — `transform()` returns the same `imports`/`exports` after stripping.
 */
export async function analyze(source: string, filename = 'input.jsx', options?: SwcOptions): Promise<Analysis> {
  return unwrap<Analysis>(await requireNative().analyze(...transformArgs(source, filename, options)));
}

/** Synchronous `analyze` — blocks the JS thread. */
export function analyzeSync(source: string, filename = 'input.jsx', options?: SwcOptions): Analysis {
  return unwrap<Analysis>(requireNative().analyzeSync(...transformArgs(source, filename, options)));
}
