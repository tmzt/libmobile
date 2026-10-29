package dev.highbay.libmobile.media

import android.app.Activity
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ComponentName
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.ExifInterface
import android.net.Uri
import android.os.Build
import android.os.ext.SdkExtensions
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import android.webkit.MimeTypeMap
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.io.File
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.util.concurrent.Executors

/**
 * Asking the user for media through Android's own chooser (Cargo feature
 * `media-request`; `libmobile::android::media`).
 *
 * Rust makes every decision (which screen, the MIME types, the limits) and
 * calls [request] from any thread; on the UI thread the plugin builds
 * `ACTION_GET_CONTENT` in `Intent.createChooser`, with one camera entry per
 * camera app writing into `<cacheDir>/libmobile-media/` through
 * [MediaFileProvider], or `ACTION_PICK_IMAGES` (the Photo Picker), and
 * starts it with its own request code. Framework only, no Jetpack.
 *
 * [onActivityResult] hands the answer to one background thread
 * (`libmobile-media`): each returned item is copied to the cache under the
 * size cap, and one `nativeMediaResult` call reports them. Errors are
 * `"<kind>: <detail>"`: `busy`, `not-visible`, `no-app`, `cancelled`,
 * `no_item`, `lost`, `platform: <exception class>`; per item `too-large` or
 * `unreadable: <exception class>`. A URI, a display name or an exception's
 * message is never logged.
 *
 * Before the chooser opens, a pending record (`<cacheDir>/libmobile-media/
 * pending`) is written, so an answer that reaches a new process (the old
 * one killed while the camera was up) is still handled, reported as
 * restored.
 */
object MediaRequestPlugin : MobileActivityPlugin {
    /** Distinct from `CameraIntentPlugin`'s 0x4c4d and `PermissionPlugin`'s 0x504d. */
    private const val REQUEST_CODE = 0x4d52 // "MR"

    /** The cache subdirectory; keep in step with `MEDIA_DIR` (src/media.rs). */
    const val DIR = "libmobile-media"
    private const val PENDING = "pending"

    /** Keep in step with `MEDIA_COPY_CHUNK` and `MEDIA_CACHE_MAX_AGE` (src/media.rs). */
    private const val COPY_CHUNK = 64 * 1024
    private const val CACHE_MAX_AGE_MS = 24L * 60 * 60 * 1000
    private const val MAX_ITEMS = 100

    private const val SOURCE_PICKED = 0
    private const val SOURCE_CAMERA = 1
    private const val CAMERA_PHOTO = 1
    private const val CAMERA_VIDEO = 2

    private const val GRANT = Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION

    private val NAME_OK = Regex("^[A-Za-z0-9]{1,10}$")

    /** One request's settings, in memory and in the pending record. */
    internal class Request(
        val id: Int,
        val maxItems: Int,
        val maxBytes: Long,
        /** 0 bytes, 1 file. */
        val delivery: Int,
        /** 0 chooser, 1 Photo Picker. */
        val picker: Int,
        val cameraLeftOut: Boolean,
        /** The camera entries' output file names. */
        val photo: String?,
        val video: String?,
        val time: Long,
    )

    /** The request whose chooser or picker is up. UI thread only. */
    private var inFlight: Request? = null

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "libmobile-media") }

    /** Called from Rust via JNI: static, `()Z`. API 33+, or API 30+ with SDK extension R >= 2. */
    @JvmStatic
    fun photoPickerAvailable(): Boolean = when {
        Build.VERSION.SDK_INT >= 33 -> true
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.R ->
            SdkExtensions.getExtensionVersion(Build.VERSION_CODES.R) >= 2
        else -> false
    }

    /**
     * Whether the app's own manifest requests [permission]. Called from Rust
     * via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`.
     */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun declares(activity: Activity, permission: String): Boolean = try {
        val pm = activity.packageManager
        val info = if (Build.VERSION.SDK_INT >= 33) {
            pm.getPackageInfo(
                activity.packageName,
                PackageManager.PackageInfoFlags.of(PackageManager.GET_PERMISSIONS.toLong()),
            )
        } else {
            pm.getPackageInfo(activity.packageName, PackageManager.GET_PERMISSIONS)
        }
        info.requestedPermissions?.contains(permission) == true
    } catch (e: Exception) {
        Log.w(NativeLib.TAG, "media: manifest check failed: ${e.javaClass.name}")
        false
    }

    /**
     * Called from Rust via JNI: static,
     * `(Landroid/app/Activity;I[IJLjava/lang/String;[Ljava/lang/String;Ljava/lang/String;)V`.
     * [ints] is `[photoPicker, maxItems, delivery, camera bits (1 photo, 2
     * video), cameraLeftOut]`; [mimeType] is `GET_CONTENT`'s type or the
     * Photo Picker's (null: its default); [mimes] is `EXTRA_MIME_TYPES`.
     */
    @JvmStatic
    fun request(
        activity: Activity,
        id: Int,
        ints: IntArray,
        maxBytes: Long,
        mimeType: String?,
        mimes: Array<String>,
        title: String?,
    ) {
        activity.runOnUiThread { start(activity, id, ints, maxBytes, mimeType, mimes, title) }
    }

    private fun start(
        activity: Activity,
        id: Int,
        ints: IntArray,
        maxBytes: Long,
        mimeType: String?,
        mimes: Array<String>,
        title: String?,
    ) {
        if (inFlight != null) {
            fail(id, "busy")
            return
        }
        notVisible()?.let {
            fail(id, it)
            return
        }
        val photoPicker = ints[0] == 1
        val maxItems = ints[1].coerceIn(1, MAX_ITEMS)
        val camera = ints[3]
        var cameraLeftOut = ints[4] == 1
        val dir = File(activity.cacheDir, DIR)
        var photo: String? = null
        var video: String? = null
        try {
            dir.mkdirs()
            prune(dir)
            val intent: Intent
            if (photoPicker) {
                intent = Intent(MediaStore.ACTION_PICK_IMAGES)
                mimeType?.let { intent.type = it }
                if (maxItems > 1) {
                    val limit = try {
                        MediaStore.getPickImagesMaxLimit()
                    } catch (e: Throwable) {
                        maxItems
                    }
                    intent.putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, minOf(maxItems, limit))
                }
            } else {
                val content = Intent(Intent.ACTION_GET_CONTENT)
                    .addCategory(Intent.CATEGORY_OPENABLE)
                    .setType(mimeType ?: "*/*")
                if (mimes.isNotEmpty()) content.putExtra(Intent.EXTRA_MIME_TYPES, mimes)
                if (maxItems > 1) content.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                val initial = mutableListOf<Intent>()
                if (camera and CAMERA_PHOTO != 0) {
                    photo = cameraEntries(activity, dir, MediaStore.ACTION_IMAGE_CAPTURE, "cam-$id.jpg", initial)
                    if (photo == null) cameraLeftOut = true
                }
                if (camera and CAMERA_VIDEO != 0) {
                    video = cameraEntries(activity, dir, MediaStore.ACTION_VIDEO_CAPTURE, "cam-$id.mp4", initial)
                    if (video == null) cameraLeftOut = true
                }
                intent = Intent.createChooser(content, title)
                if (initial.isNotEmpty()) intent.putExtra(Intent.EXTRA_INITIAL_INTENTS, initial.toTypedArray())
            }
            val request = Request(
                id, maxItems, maxBytes, ints[2], if (photoPicker) 1 else 0,
                cameraLeftOut, photo, video, System.currentTimeMillis(),
            )
            writePending(dir, request)
            inFlight = request
            @Suppress("DEPRECATION")
            activity.startActivityForResult(intent, REQUEST_CODE)
            Log.i(NativeLib.TAG, "media: request $id started (picker ${request.picker}, camera $camera)")
        } catch (e: ActivityNotFoundException) {
            abandon(activity, dir, photo, video)
            fail(id, "no-app")
        } catch (e: Exception) {
            abandon(activity, dir, photo, video)
            fail(id, platform(e))
        }
    }

    /**
     * One initial intent per activity that handles [action], each given
     * write access to the output URI of [name]; the file's name, or null when
     * no camera app is found (no entry).
     */
    @Suppress("DEPRECATION")
    private fun cameraEntries(
        activity: Activity,
        dir: File,
        action: String,
        name: String,
        into: MutableList<Intent>,
    ): String? {
        val apps = activity.packageManager.queryIntentActivities(Intent(action), 0)
        if (apps.isEmpty()) return null
        val file = File(dir, name)
        file.delete()
        file.createNewFile()
        val uri = MediaFileProvider.uriFor(activity.packageName, name)
        for (app in apps) {
            val info = app.activityInfo ?: continue
            // Explicit: the chooser, not this activity, starts the camera,
            // and the intent's flags are not always carried through.
            activity.grantUriPermission(info.packageName, uri, GRANT)
            val entry = Intent(action)
                .setComponent(ComponentName(info.packageName, info.name))
                .putExtra(MediaStore.EXTRA_OUTPUT, uri)
                .addFlags(GRANT)
            entry.clipData = ClipData.newRawUri("", uri)
            into += entry
        }
        return name
    }

    override fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CODE) return false
        val context = activity.applicationContext
        val dir = File(activity.cacheDir, DIR)
        val memory = inFlight
        inFlight = null
        // Read and removed here, on the UI thread, so a request started
        // before the copy ends cannot lose its own record.
        val request = memory ?: readPending(dir)
        File(dir, PENDING).delete()
        val restored = memory == null
        if (request == null) {
            Log.w(NativeLib.TAG, "media: an answer with no request or pending record")
            executor.execute { deliver(0, true, "lost", 0, 0, false, false, Items()) }
            return true
        }
        executor.execute { answer(context, dir, request, restored, resultCode, data) }
        return true
    }

    /** The answer, on `libmobile-media`. */
    private fun answer(context: Context, dir: File, r: Request, restored: Boolean, resultCode: Int, data: Intent?) {
        val cameraNames = listOfNotNull(r.photo, r.video)
        val ours = cameraNames.map { MediaFileProvider.uriFor(context.packageName, it) }
        val keep = mutableSetOf<String>()
        try {
            if (resultCode != Activity.RESULT_OK) {
                deliver(r, restored, "cancelled", false, Items())
                return
            }
            val uris = mutableListOf<Uri>()
            data?.clipData?.let { clip ->
                for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let { uris += it }
            }
            if (uris.isEmpty()) data?.data?.let { uris += it }
            // Our own output URI, echoed by a camera app, is not a picked item.
            uris.removeAll { it in ours }
            val items = Items()
            var truncated = false
            if (uris.isEmpty()) {
                for (name in cameraNames) {
                    val file = File(dir, name)
                    val size = file.length()
                    if (size <= 0 || items.size >= r.maxItems) continue
                    val uri = MediaFileProvider.uriFor(context.packageName, name).toString()
                    val mime = if (name.endsWith(".jpg")) "image/jpeg" else "video/mp4"
                    if (size > r.maxBytes) {
                        items.add(SOURCE_CAMERA, uri, mime, name, size, null, "too-large", 0)
                    } else {
                        keep += name
                        items.add(SOURCE_CAMERA, uri, mime, name, size, file.path, null, exif(mime, file))
                    }
                }
            } else {
                truncated = uris.size > r.maxItems
                val cameraWritten = cameraNames.any { File(dir, it).length() > 0 }
                for ((n, uri) in uris.take(r.maxItems).withIndex()) {
                    copyItem(context.contentResolver, dir, r, n, uri, items) { mime ->
                        // A camera app that ignores EXTRA_OUTPUT for video
                        // and returns its own URI.
                        uris.size == 1 && !cameraWritten && r.video != null && mime?.startsWith("video/") == true
                    }
                }
            }
            if (items.size == 0) {
                deliver(r, restored, "no_item", false, items)
            } else {
                deliver(r, restored, null, truncated, items)
            }
        } catch (e: Throwable) {
            deliver(r, restored, platform(e), false, Items())
        } finally {
            for (uri in ours) {
                try {
                    context.revokeUriPermission(uri, GRANT)
                } catch (e: Exception) {
                    Log.w(NativeLib.TAG, "media: revoke failed: ${e.javaClass.name}")
                }
            }
            for (name in cameraNames) if (name !in keep) File(dir, name).delete()
        }
    }

    private class Meta(val mime: String?, val name: String?, val size: Long?)

    private fun meta(resolver: ContentResolver, uri: Uri): Meta {
        val mime = try {
            resolver.getType(uri)
        } catch (e: Exception) {
            null
        }
        var name: String? = null
        var size: Long? = null
        try {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
                ?.use { c ->
                    if (c.moveToFirst()) {
                        val ni = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        if (ni >= 0 && !c.isNull(ni)) name = c.getString(ni)
                        val si = c.getColumnIndex(OpenableColumns.SIZE)
                        if (si >= 0 && !c.isNull(si)) size = c.getLong(si)
                    }
                }
        } catch (e: Exception) {
            // The name and size are optional.
        }
        return Meta(mime, name, size)
    }

    /** Copy one picked item to `<id>-<n>.<ext>` under the cap. */
    private fun copyItem(
        resolver: ContentResolver,
        dir: File,
        r: Request,
        n: Int,
        uri: Uri,
        items: Items,
        isCamera: (String?) -> Boolean,
    ) {
        val meta = meta(resolver, uri)
        val source = if (isCamera(meta.mime)) SOURCE_CAMERA else SOURCE_PICKED
        val size = meta.size
        val text = uri.toString()
        if (size != null && size > r.maxBytes) {
            items.add(source, text, meta.mime, meta.name, size, null, "too-large", 0)
            return
        }
        val file = File(dir, "${r.id}-$n.${extension(meta.mime)}")
        try {
            if (copy(resolver, uri, file, r.maxBytes)) {
                file.delete()
                items.add(source, text, meta.mime, meta.name, size ?: -1, null, "too-large", 0)
            } else {
                items.add(source, text, meta.mime, meta.name, size ?: file.length(), file.path, null, exif(meta.mime, file))
            }
        } catch (e: Throwable) {
            file.delete()
            items.add(source, text, meta.mime, meta.name, size ?: -1, null, "unreadable: " + e.javaClass.name, 0)
        }
    }

    /** Stream [uri] into [file] in [COPY_CHUNK]s; true when it passed [max] (the copy stops). */
    private fun copy(resolver: ContentResolver, uri: Uri, file: File, max: Long): Boolean {
        val input = resolver.openInputStream(uri) ?: throw FileNotFoundException()
        input.use { inp ->
            FileOutputStream(file).use { out ->
                val buf = ByteArray(COPY_CHUNK)
                var total = 0L
                while (true) {
                    val read = inp.read(buf)
                    if (read < 0) break
                    total += read
                    if (total > max) return true
                    out.write(buf, 0, read)
                }
            }
        }
        return false
    }

    private fun exif(mime: String?, file: File): Int {
        if (mime != "image/jpeg") return 0
        return try {
            ExifInterface(file.path).getAttributeInt(ExifInterface.TAG_ORIENTATION, 0).takeIf { it in 0..8 } ?: 0
        } catch (e: Exception) {
            0
        }
    }

    private fun extension(mime: String?): String {
        val ext = mime?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }
        return if (ext != null && NAME_OK.matches(ext)) ext else "bin"
    }

    /** Files in the media directory older than [CACHE_MAX_AGE_MS]. */
    private fun prune(dir: File) {
        val before = System.currentTimeMillis() - CACHE_MAX_AGE_MS
        dir.listFiles()?.forEach { if (it.isFile && it.lastModified() < before) it.delete() }
    }

    /** A request that did not start: its camera files and grants go. */
    private fun abandon(context: Context, dir: File, photo: String?, video: String?) {
        inFlight = null
        File(dir, PENDING).delete()
        for (name in listOfNotNull(photo, video)) {
            try {
                context.revokeUriPermission(MediaFileProvider.uriFor(context.packageName, name), GRANT)
            } catch (e: Exception) {
                // Nothing was granted.
            }
            File(dir, name).delete()
        }
    }

    /**
     * Android 10+ refuses, silently, an activity start from an app that is
     * not visible (as `LinkPlugin`).
     */
    private fun notVisible(): String? {
        val state = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(state)
        if (state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND &&
            state.importance != ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
        ) {
            return "not-visible: importance ${state.importance}"
        }
        return null
    }

    private fun platform(e: Throwable): String = "platform: " + e.javaClass.name

    // ── The pending record ───────────────────────────────────────────────

    private fun writePending(dir: File, r: Request) {
        val text = buildString {
            append("id=${r.id}\n")
            append("max_items=${r.maxItems}\n")
            append("max_bytes=${r.maxBytes}\n")
            append("delivery=${r.delivery}\n")
            append("picker=${r.picker}\n")
            append("camera_left_out=${if (r.cameraLeftOut) 1 else 0}\n")
            r.photo?.let { append("photo=$it\n") }
            r.video?.let { append("video=$it\n") }
            append("time=${r.time}\n")
        }
        File(dir, PENDING).writeText(text)
    }

    private fun readPending(dir: File): Request? = try {
        parsePending(File(dir, PENDING).readText())
    } catch (e: Exception) {
        null
    }

    /**
     * Keep in step with `parse_pending` (src/media.rs): `id` (> 0),
     * `max_items` (1..100), `max_bytes` (> 0), `delivery` and `picker` (0
     * or 1) are required; `camera_left_out` (0 or 1), `photo`, `video`
     * (names passing [MediaFileProvider.nameOk]) and `time` are optional;
     * unknown keys are ignored. Anything else: null.
     */
    internal fun parsePending(text: String): Request? {
        val map = HashMap<String, String>()
        for (raw in text.lines()) {
            val line = raw.trim()
            if (line.isEmpty()) continue
            val at = line.indexOf('=')
            if (at < 0) return null
            map[line.substring(0, at).trim()] = line.substring(at + 1).trim()
        }
        fun flag(key: String): Int? = when (map[key]) {
            null, "0" -> 0
            "1" -> 1
            else -> null
        }
        val id = map["id"]?.toIntOrNull()?.takeIf { it > 0 } ?: return null
        val maxItems = map["max_items"]?.toIntOrNull()?.takeIf { it in 1..MAX_ITEMS } ?: return null
        val maxBytes = map["max_bytes"]?.toLongOrNull()?.takeIf { it > 0 } ?: return null
        if (map["delivery"] == null || map["picker"] == null) return null
        val delivery = flag("delivery") ?: return null
        val picker = flag("picker") ?: return null
        val cameraLeftOut = flag("camera_left_out") ?: return null
        val photo = map["photo"]
        val video = map["video"]
        if (photo != null && !MediaFileProvider.nameOk(photo)) return null
        if (video != null && !MediaFileProvider.nameOk(video)) return null
        val time = map["time"]?.let { it.toLongOrNull() ?: return null } ?: 0L
        return Request(id, maxItems, maxBytes, delivery, picker, cameraLeftOut == 1, photo, video, time)
    }

    // ── Reporting ────────────────────────────────────────────────────────

    /** The items, as parallel arrays for one JNI call. */
    private class Items {
        val sources = mutableListOf<Int>()
        val uris = mutableListOf<String?>()
        val mimes = mutableListOf<String?>()
        val names = mutableListOf<String?>()
        val sizes = mutableListOf<Long>()
        val paths = mutableListOf<String?>()
        val errors = mutableListOf<String?>()
        val exif = mutableListOf<Int>()
        val size: Int get() = sources.size

        fun add(source: Int, uri: String, mime: String?, name: String?, size: Long, path: String?, error: String?, orientation: Int) {
            sources += source
            uris += uri
            mimes += mime
            names += name
            sizes += size
            paths += path
            errors += error
            exif += orientation
        }
    }

    /** An error before the chooser opened, from the UI thread. */
    private fun fail(id: Int, error: String) {
        Log.i(NativeLib.TAG, "media: request $id: ${error.substringBefore(':')}")
        deliver(id, false, error, 0, 0, false, false, Items())
    }

    private fun deliver(r: Request, restored: Boolean, error: String?, truncated: Boolean, items: Items) {
        deliver(r.id, restored, error, r.picker, r.delivery, r.cameraLeftOut, truncated, items)
    }

    private fun deliver(
        id: Int,
        restored: Boolean,
        error: String?,
        picker: Int,
        delivery: Int,
        cameraLeftOut: Boolean,
        truncated: Boolean,
        items: Items,
    ) {
        try {
            nativeMediaResult(
                id, restored, error, picker, delivery, cameraLeftOut, truncated,
                items.sources.toIntArray(),
                items.uris.toTypedArray(),
                items.mimes.toTypedArray(),
                items.names.toTypedArray(),
                items.sizes.toLongArray(),
                items.paths.toTypedArray(),
                items.errors.toTypedArray(),
                items.exif.toIntArray(),
            )
        } catch (e: UnsatisfiedLinkError) {
            Log.e(NativeLib.TAG, "media: native callback unavailable", e)
        }
    }

    /**
     * Implemented in Rust (`src/android/media.rs`) as
     * `Java_dev_highbay_libmobile_media_MediaRequestPlugin_nativeMediaResult`.
     */
    private external fun nativeMediaResult(
        id: Int,
        restored: Boolean,
        error: String?,
        picker: Int,
        delivery: Int,
        cameraLeftOut: Boolean,
        truncated: Boolean,
        sources: IntArray,
        uris: Array<String?>,
        mimes: Array<String?>,
        names: Array<String?>,
        sizes: LongArray,
        paths: Array<String?>,
        itemErrors: Array<String?>,
        exif: IntArray,
    )
}
