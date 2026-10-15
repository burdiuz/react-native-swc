# Changelog

## 0.1.0

Initial release.

- `transform` / `transformSync` — TypeScript, JSX (automatic runtime) and ES module → CommonJS with
  source maps. Default config lowers only what Hermes' compiler rejects (classes, async functions
  and arrows, async generators, `for await`, class fields / private members / static blocks).
- `transform` also returns the module's `imports` and `exports` (measured after TypeScript stripping).
- `minify` / `minifySync`, `parse` / `parseSync`, `print` / `printSync`, `analyze` / `analyzeSync`.
- Syntax errors are reported as `file:line:col: message`.
- SWC panics are caught and reported as errors (`panic = "unwind"` in the release profile).
- Android `arm64-v8a` prebuilt library.
