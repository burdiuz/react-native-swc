# @actualwave/react-native-swc

On-device [SWC](https://swc.rs) for React Native (**Android**). Compile TypeScript / JSX / modern
JavaScript, minify, parse, print and analyse imports **inside the app** — no Metro, no network.

It exists to feed code evaluation on Hermes: Hermes' runtime compiler rejects `class`, async arrow
functions, async generators and `for await`, and has no module system. This package turns user
source into something Hermes accepts. It is the optional companion of
`@actualwave/react-native-hermes-eval`, and is a separate package so apps that don't need it don't
pay for a ~13 MB native library (arm64).

- TypeScript + JSX (automatic runtime) → CommonJS, with source maps
- Lowers **only** what Hermes rejects; everything else stays native (small, fast output)
- `imports` / `exports` of every transformed module — no separate lexer pass
- Minify, parse to AST, print an AST, analyse without transforming
- Async and synchronous variants of everything
- SWC errors reported as `file:line:col: message`; SWC panics reported as errors

> Android only. `isAvailable()` is `false` on iOS, web and Expo Go.

## Install

```bash
npm install @actualwave/react-native-swc
```

The module is autolinked on Android. It contains native code, so:

- **Bare React Native** — rebuild the app (`npx react-native run-android`).
- **Expo** — use a development build / `expo prebuild`; it does **not** work in Expo Go.

## Quick start

```ts
import { isAvailable, transform } from '@actualwave/react-native-swc';

if (isAvailable()) {
  const { code, map, imports, exports } = await transform(source, 'App.tsx');
  // code    — CommonJS, ready for Hermes
  // map     — source map v3 JSON string
  // imports — [{ specifier: 'react', kind: 'import', names: ['default', 'useState'] }, …]
  // exports — ['default', 'App', …]
}
```

Evaluating the result is up to you. With `@actualwave/react-native-hermes-eval`:

```ts
import { evaluateJavaScript } from '@actualwave/react-native-hermes-eval';

const wrapped = `(function(require){var module={exports:{}};var exports=module.exports;${code}\nreturn module.exports;})`;
const factory = evaluateJavaScript(wrapped, 'user_code.js') as (
  require: (specifier: string) => unknown,
) => Record<string, unknown>;
const mod = factory((specifier) => resolve(specifier)); // you provide `resolve`
```

Use `imports` to know which modules to load/resolve before running the code.

## API

All functions take `source` and an optional `filename` (used for the error prefix, the source map,
and to choose the parser: `.ts`/`.mts`/`.cts` → TypeScript, `.tsx` → TSX, anything else →
JavaScript with JSX). Each has a `…Sync` variant that **blocks the JS thread**; prefer the async
form for anything larger than a snippet.

| Function | Result | Purpose |
| --- | --- | --- |
| `isAvailable()` | `boolean` | Native module linked (Android) |
| `transform(source, filename?, options?)` | `{ code, map?, imports, exports }` | Compile TS / JSX / ESM |
| `minify(source, filename?, options?)` | `{ code, map? }` | Compress + mangle **plain JavaScript** |
| `parse(source, filename?, options?)` | `Program` (AST) | Parse without transforming |
| `print(ast, options?)` | `{ code }` | Print an AST back to code |
| `analyze(source, filename?, options?)` | `{ imports, exports }` | Import/export analysis, no transform |
| `defaultOptions(filename)` | `SwcOptions` | The options `transform` starts from |

Sync forms: `transformSync`, `minifySync`, `parseSync`, `printSync`, `analyzeSync`.

### `transform`

```ts
transform(source: string, filename = 'input.jsx', options?: SwcOptions): Promise<TransformResult>
```

`options` is **deep-merged** over `defaultOptions(filename)` (objects merge; arrays and scalars
replace). Any [SWC `Options`](https://swc.rs/docs/configuration/compilation) key is accepted.

Defaults (`defaultOptions`):

```jsonc
{
  "jsc": {
    "parser": { "syntax": "typescript", "tsx": true },   // or ecmascript + jsx; chosen by extension
    "transform": { "react": { "runtime": "automatic" } },
    "externalHelpers": false,                            // helpers are inlined per module
    "keepClassNames": true
  },
  "env": {
    "targets": "chrome 110",
    "include": [                                         // lowered because Hermes rejects them
      "transform-classes", "transform-async-to-generator", "transform-async-generator-functions",
      "transform-class-properties", "transform-private-methods",
      "transform-private-property-in-object", "transform-class-static-block"
    ]
  },
  "module": { "type": "commonjs" },
  "sourceMaps": true,
  "isModule": true,
  "swcrc": false, "configFile": false
}
```

Notes:

- **`env` and `jsc.target` are mutually exclusive in SWC.** The defaults use `env` so only the listed
  features are lowered. To target something else, replace the whole decision, e.g.
  `{ env: { targets: 'chrome 80' } }` lowers more syntax, or set `jsc.target` *and* clear `env`.
- Because arrays replace, passing `env: { include: [...] }` **replaces** the default feature list.
- The lowered feature list was chosen by compiling sample code with Hermes' compiler (`hermesc`):
  those constructs are rejected, everything else tried (arrow functions, generators, destructuring,
  optional chaining, logical assignment, …) was accepted. This is a syntax check only — no
  polyfills are added, and runtime API availability is up to the engine.
- `jsx` defaults to the automatic runtime, so the output requires `react/jsx-runtime`.

**`imports`** lists, in source order, each dependency with a string-literal specifier:

| `kind` | From |
| --- | --- |
| `import` | `import … from 'x'` and `import 'x'` |
| `export-from` | `export { a } from 'x'` |
| `export-all` | `export * from 'x'` |
| `dynamic` | `import('x')` |
| `require` | `require('x')` |

`names` holds the bound names (`default`, `*` for a namespace, named imports/re-exports) and is
omitted when empty. `exports` lists exported binding names, `default` included.

Measured **after TypeScript stripping**: `import type …`, type-only specifiers, and value imports
used only as types are not listed. Not detected: non-literal `import(name)` / `require(name)`.

### `minify`

```ts
minify(source: string, filename = 'input.js', options?: MinifyOptions): Promise<{ code: string; map?: string }>
```

Defaults to `compress` + `mangle`, no source map. Input must be plain JavaScript — run `transform`
first for TS/JSX. [Options reference](https://swc.rs/docs/configuration/minification).

### `parse` / `print`

```ts
const ast = await parse(source, 'file.tsx');          // Program (SWC AST, JSON)
const { code } = await print(ast, { target: 'es2022', minify: false, asciiOnly: false });
```

`parse` runs no transforms. Parser settings come from `defaultOptions(filename)`
(`options.jsc.parser` / `jsc.target` / `isModule` to override). `print` re-inserts parentheses a
hand-edited tree may be missing.

- **Comments and source maps are not preserved** — the AST carries neither.
- The AST crosses the bridge as one JSON string, which is expensive for large files.
- The AST shape follows `swc_ecma_ast` serialisation of the bundled SWC version; it is not stable
  across SWC upgrades.

### `analyze`

Same `imports` / `exports` as `transform`, without transforming or generating code. TypeScript is
**not stripped** here, so value imports used only as types are still listed (type-only
`import type` are skipped). Use `transform`'s result when you're transforming anyway.

### Errors

Failures throw `Error` (async forms reject). A located syntax error looks like:

```
src/App.jsx:2:9: Expression expected
```

Several diagnostics are joined with newlines. Invalid options / AST JSON and an SWC panic
(`swc panicked: <message>`) are reported the same way.

## Supported platforms and ABIs

| | |
| --- | --- |
| Android `arm64-v8a` | prebuilt, shipped in the package |
| Android `armeabi-v7a`, `x86_64`, `x86` | **not built by default** |
| iOS / web | not supported (`isAvailable() === false`) |

The library is loaded when the native module initialises. On an ABI without a matching `.so` the
app **crashes at launch** with `UnsatisfiedLinkError`; build the ABIs you need
(see below) or restrict your app's `abiFilters`.

Size: the `arm64-v8a` library is ~13 MB before APK compression (the SWC minifier and transforms
are statically linked); other ABIs were not measured.

## Behaviour and limits

- **Threading.** One worker thread with a 64 MB stack serializes all calls. Deep nesting is handled
  by that stack; a genuine native stack overflow or an out-of-memory condition still terminates
  the process (those are not panics and cannot be caught).
- **Sync methods.** `…Sync` uses blocking synchronous native methods. They block the JS thread for
  the duration of the call, and require a JS engine that supports them (not remote JS debugging).
- **Panics.** The release profile uses `panic = "unwind"` so an internal SWC panic becomes a normal
  error instead of killing the app.
- **No caching.** Every call compiles from scratch; cache by content hash yourself if you re-run
  unchanged files.

## Building the native library

Needs `rustup`, [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk) (`cargo install cargo-ndk`) and
the Android NDK (`ANDROID_NDK_HOME`).

```bash
npm run build:android                              # arm64-v8a only
npm run build:android -- arm64-v8a x86_64          # choose ABIs
npm run test:rust                                  # Rust unit tests (host)
npm run typecheck
```

Output lands in `android/src/main/jniLibs/<abi>/libactualwave_swc.so`. It is git-ignored but
included in the npm tarball.

### Releasing

```bash
npm run licenses          # regenerate THIRD_PARTY_LICENSES.md after changing Rust dependencies
npm run build:android -- arm64-v8a armeabi-v7a x86_64
npm pack --dry-run        # inspect the tarball
```

`prepublishOnly` runs the type-check, the Rust tests, and a guard that fails when no prebuilt
library exists or the library is older than `rust/`.

## Layout

```
src/index.ts                         TypeScript API + default options
android/…/SwcModule.kt               React Native module (worker thread, JNI declarations)
rust/src/lib.rs                      JSON-in / JSON-out SWC wrapper + JNI entry points
rust/src/analyze.rs                  import / export collector
scripts/                             build, license generation, prepublish guard
```

## License

[MIT](LICENSE). The prebuilt library statically links [SWC](https://github.com/swc-project/swc)
(Apache-2.0) and other third-party Rust crates — see
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
