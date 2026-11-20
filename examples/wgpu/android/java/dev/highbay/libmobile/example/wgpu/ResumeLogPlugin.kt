package dev.highbay.libmobile.example.wgpu

import android.app.Activity
import android.util.Log
import dev.highbay.libmobile.MobileActivityPlugin

/**
 * The example's own plugin, registered from its build.rs with
 * `Project::plugin(AppPlugin::new("dev.highbay.libmobile.example.wgpu.ResumeLogPlugin"))`.
 * It runs after libmobile's plugins on every resume, logs it, and tells the
 * example's cdylib, which exports [nativeResumed].
 */
object ResumeLogPlugin : MobileActivityPlugin {
    private const val TAG = "libmobile-wgpu"
    private var resumes = 0

    override fun onResume(activity: Activity) {
        resumes += 1
        Log.i(TAG, "ResumeLogPlugin: onResume #$resumes")
        nativeResumed(resumes)
    }

    /** `Java_dev_highbay_libmobile_example_wgpu_ResumeLogPlugin_nativeResumed` in the example's `.so`. */
    private external fun nativeResumed(count: Int)
}
