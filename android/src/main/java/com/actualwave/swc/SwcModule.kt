package com.actualwave.swc

import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import java.util.concurrent.Callable
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * React Native bridge to the SWC (Rust) shared library `libactualwave_swc.so`.
 *
 * Every method exchanges plain strings with the native side: source / AST / options go in as
 * strings (options and ASTs as JSON), and a JSON string comes back that is either the result
 * object or `{"error": "<message>"}`. The TypeScript wrapper (`src/index.ts`) builds the options
 * and turns `{"error"}` into a thrown `Error`.
 *
 * Threading: all work runs on a single worker thread with a 64 MB stack (SWC recurses deeply on
 * large inputs; default thread stacks are too small), so concurrent calls are serialized. The
 * `*Sync` methods block the calling (JS) thread until the worker is done.
 *
 * The library is loaded when this class is first initialised; if the `.so` for the device ABI is
 * missing this throws `UnsatisfiedLinkError` and the app fails fast on launch (see README,
 * "Supported ABIs").
 */
class SwcModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
  override fun getName() = "ActualwaveSwc"

  /** Transforms `source` with SWC `Options` JSON. Resolves `{"code","map","imports","exports"}`. */
  @ReactMethod
  fun transform(source: String, filename: String, optionsJson: String, promise: Promise) =
    submit(promise) { nativeTransform(source, filename, optionsJson) }

  /** Blocking variant of [transform]. */
  @ReactMethod(isBlockingSynchronousMethod = true)
  fun transformSync(source: String, filename: String, optionsJson: String): String =
    runBlocking { nativeTransform(source, filename, optionsJson) }

  /** Minifies plain JavaScript with SWC `JsMinifyOptions` JSON. Resolves `{"code","map"}`. */
  @ReactMethod
  fun minify(source: String, filename: String, optionsJson: String, promise: Promise) =
    submit(promise) { nativeMinify(source, filename, optionsJson) }

  /** Blocking variant of [minify]. */
  @ReactMethod(isBlockingSynchronousMethod = true)
  fun minifySync(source: String, filename: String, optionsJson: String): String =
    runBlocking { nativeMinify(source, filename, optionsJson) }

  /** Parses `source` to an SWC AST without transforming it. Resolves `{"ast": Program}`. */
  @ReactMethod
  fun parse(source: String, filename: String, optionsJson: String, promise: Promise) =
    submit(promise) { nativeParse(source, filename, optionsJson) }

  /** Blocking variant of [parse]. */
  @ReactMethod(isBlockingSynchronousMethod = true)
  fun parseSync(source: String, filename: String, optionsJson: String): String =
    runBlocking { nativeParse(source, filename, optionsJson) }

  /** Prints an AST (JSON, as returned by [parse]) back to code. Resolves `{"code"}`. */
  @ReactMethod
  fun print(astJson: String, optionsJson: String, promise: Promise) =
    submit(promise) { nativePrint(astJson, optionsJson) }

  /** Blocking variant of [print]. */
  @ReactMethod(isBlockingSynchronousMethod = true)
  fun printSync(astJson: String, optionsJson: String): String =
    runBlocking { nativePrint(astJson, optionsJson) }

  /** Lists imports / exports of `source` without transforming it. Resolves `{"imports","exports"}`. */
  @ReactMethod
  fun analyze(source: String, filename: String, optionsJson: String, promise: Promise) =
    submit(promise) { nativeAnalyze(source, filename, optionsJson) }

  /** Blocking variant of [analyze]. */
  @ReactMethod(isBlockingSynchronousMethod = true)
  fun analyzeSync(source: String, filename: String, optionsJson: String): String =
    runBlocking { nativeAnalyze(source, filename, optionsJson) }

  /** Runs [work] on the worker and settles [promise] with its result. */
  private fun submit(promise: Promise, work: () -> String) {
    EXECUTOR.execute {
      try {
        promise.resolve(work())
      } catch (t: Throwable) {
        promise.reject("E_SWC", t)
      }
    }
  }

  /** Runs [work] on the worker and waits for it (used by the `*Sync` methods). */
  private fun runBlocking(work: () -> String): String =
    EXECUTOR.submit(Callable { work() }).get()

  companion object {
    /** Single big-stack worker; daemon so it never keeps the process alive. */
    private val EXECUTOR: ExecutorService = Executors.newSingleThreadExecutor { r ->
      Thread(null, r, "swc-worker", 64L * 1024 * 1024).apply { isDaemon = true }
    }

    init {
      System.loadLibrary("actualwave_swc")
    }

    // JNI entry points, implemented in rust/src/lib.rs. Each returns a JSON string.
    @JvmStatic
    external fun nativeTransform(source: String, filename: String, optionsJson: String): String

    @JvmStatic
    external fun nativeMinify(source: String, filename: String, optionsJson: String): String

    @JvmStatic
    external fun nativeParse(source: String, filename: String, optionsJson: String): String

    @JvmStatic
    external fun nativeAnalyze(source: String, filename: String, optionsJson: String): String

    @JvmStatic
    external fun nativePrint(astJson: String, optionsJson: String): String
  }
}
