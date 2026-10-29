package dev.highbay.libmobile.media

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException

/**
 * libmobile's own content provider for camera output (Cargo feature
 * `media-request`), instead of AndroidX `FileProvider`: framework only, one
 * directory, no XML paths resource.
 *
 * URIs are `content://<package>.libmobile.media/out/<name>`; `<name>` must
 * pass [nameOk] and is resolved to `<cacheDir>/libmobile-media/<name>`,
 * whose canonical path must stay in that directory. Anything else is a
 * [FileNotFoundException]. Declared `exported="false"` and
 * `grantUriPermissions="true"` in the generated manifest: only an app
 * holding a per-URI grant from us (the camera app the chooser starts) can
 * open a file, and only that one.
 */
class MediaFileProvider : ContentProvider() {
    companion object {
        /** The URI path's first segment. */
        const val OUT = "out"

        private val NAME = Regex("^[A-Za-z0-9._-]{1,96}$")

        /**
         * Whether [name] may be served: 1 to 96 of `A-Z a-z 0-9 . _ -`, not
         * `.` or `..`. Keep in step with `file_name_ok` (src/media.rs).
         */
        fun nameOk(name: String): Boolean = NAME.matches(name) && name != "." && name != ".."

        /** The provider's authority for this app. */
        fun authority(packageName: String): String = "$packageName.libmobile.media"

        /** The URI the provider serves `<cacheDir>/libmobile-media/<name>` at. */
        fun uriFor(packageName: String, name: String): Uri =
            Uri.Builder().scheme("content").authority(authority(packageName))
                .appendPath(OUT).appendPath(name).build()
    }

    override fun onCreate(): Boolean = true

    private fun fileFor(uri: Uri): File {
        val segments = uri.pathSegments
        if (segments.size != 2 || segments[0] != OUT || !nameOk(segments[1])) {
            throw FileNotFoundException("not a libmobile media file")
        }
        val context = context ?: throw FileNotFoundException("no context")
        val dir = File(context.cacheDir, MediaRequestPlugin.DIR).canonicalFile
        val file = File(dir, segments[1]).canonicalFile
        if (file.parentFile != dir) throw FileNotFoundException("outside the media directory")
        return file
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor =
        ParcelFileDescriptor.open(fileFor(uri), ParcelFileDescriptor.parseMode(mode))

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor {
        val file = fileFor(uri)
        val columns = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val known = columns.filter { it == OpenableColumns.DISPLAY_NAME || it == OpenableColumns.SIZE }
        val cursor = MatrixCursor(known.toTypedArray(), 1)
        cursor.addRow(
            known.map {
                if (it == OpenableColumns.DISPLAY_NAME) file.name else file.length()
            }.toTypedArray(),
        )
        return cursor
    }

    override fun getType(uri: Uri): String {
        val name = uri.lastPathSegment ?: return "application/octet-stream"
        return when {
            name.endsWith(".jpg") -> "image/jpeg"
            name.endsWith(".mp4") -> "video/mp4"
            else -> "application/octet-stream"
        }
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
}
