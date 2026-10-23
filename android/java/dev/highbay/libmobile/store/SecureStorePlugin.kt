package dev.highbay.libmobile.store

import android.app.Activity
import android.app.KeyguardManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.security.keystore.UserNotAuthenticatedException
import android.util.Log
import dev.highbay.libmobile.GeneratedPlugins
import dev.highbay.libmobile.MobileActivityPlugin
import dev.highbay.libmobile.NativeLib
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.GCMParameterSpec

/**
 * Named byte blobs, encrypted at rest (`libmobile::android::secure_store`).
 *
 * One AES-256-GCM key per app lives in Android Keystore and never leaves it:
 * in StrongBox where the device has one (unless the app was built with
 * `secure-store-no-strongbox`), else in the TEE. Each blob is encrypted on
 * its own with its name as associated data, so a file renamed to another
 * name fails to decrypt. Files go in the no-backup directory: the key does
 * not travel with a backup, so a restored blob could never be read.
 *
 * There are two keys: the default one, and one that Keystore refuses while
 * the device is locked (`setUnlockedDeviceRequired`). Bit 0 of a blob's
 * flags says which key wrapped it.
 *
 * File layout: magic `LMSS`, version 1, flags, IV length, IV, ciphertext
 * with the GCM tag at the end. The associated data is the header (up to
 * the IV) followed by the name. Writes go to a `.tmp` file that is synced
 * and then moved over the blob.
 *
 * Failures are thrown as [SecureStoreException] with a `"<kind>: <detail>"`
 * message (`keystore`, `io`, `corrupt`, `device-locked`), which Rust maps to its error type.
 * Every method is synchronous and may be called from any thread.
 */
object SecureStorePlugin : MobileActivityPlugin {
    private const val ALIAS = "libmobile.secure-store.v1"
    private const val UNLOCKED_ALIAS = "libmobile.secure-store.unlocked.v1"
    private const val PROVIDER = "AndroidKeyStore"
    private const val TRANSFORM = "AES/GCM/NoPadding"
    private const val DIR = "libmobile-secure-store"
    private const val SUFFIX = ".blob"
    private val MAGIC = byteArrayOf('L'.code.toByte(), 'M'.code.toByte(), 'S'.code.toByte(), 'S'.code.toByte())
    private const val VERSION: Byte = 1
    private const val HEADER = 7

    /** Flag bit 0: wrapped by the key that needs the device unlocked. */
    private const val FLAG_UNLOCKED = 1
    private const val TAG_BITS = 128

    /** Guards the key and the files. */
    private val lock = Any()

    class SecureStoreException(message: String, cause: Throwable? = null) : Exception(message, cause)

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)[B`. `null` if absent. */
    @JvmStatic
    fun get(activity: Activity, name: String): ByteArray? = synchronized(lock) {
        val file = blob(activity, name)
        val data = try {
            if (!file.exists()) return null
            file.readBytes()
        } catch (e: IOException) {
            throw fail("io", e)
        }
        val ivLen = parseHeader(data)
        val unlocked = data[5].toInt() and FLAG_UNLOCKED != 0
        try {
            val key = key(activity, unlocked, create = false)
                ?: throw SecureStoreException("corrupt: the store's key is missing")
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, data, HEADER, ivLen))
            cipher.updateAAD(data, 0, HEADER)
            cipher.updateAAD(name.toByteArray(Charsets.UTF_8))
            cipher.doFinal(data, HEADER + ivLen, data.size - HEADER - ivLen)
        } catch (e: UserNotAuthenticatedException) {
            throw fail("device-locked", e)
        } catch (e: AEADBadTagException) {
            throw fail("corrupt", e)
        } catch (e: KeyPermanentlyInvalidatedException) {
            throw fail("corrupt", e)
        } catch (e: GeneralSecurityException) {
            throw fail("keystore", e)
        } catch (e: ProviderException) {
            throw fail("keystore", e)
        }
    }

    /**
     * Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;[BI)V`.
     * [flags] bit 0 wraps the blob with the key that needs the device unlocked.
     */
    @JvmStatic
    fun put(activity: Activity, name: String, value: ByteArray, flags: Int): Unit = synchronized(lock) {
        val sealed = try {
            require(flags and FLAG_UNLOCKED.inv() == 0) { "unknown flags $flags" }
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.ENCRYPT_MODE, key(activity, flags and FLAG_UNLOCKED != 0, create = true))
            val iv = cipher.iv
            val header = MAGIC + byteArrayOf(VERSION, flags.toByte(), iv.size.toByte())
            cipher.updateAAD(header)
            cipher.updateAAD(name.toByteArray(Charsets.UTF_8))
            header + iv + cipher.doFinal(value)
        } catch (e: UserNotAuthenticatedException) {
            throw fail("device-locked", e)
        } catch (e: GeneralSecurityException) {
            throw fail("keystore", e)
        } catch (e: ProviderException) {
            throw fail("keystore", e)
        } finally {
            value.fill(0)
        }
        val file = blob(activity, name)
        val tmp = File(file.path + ".tmp")
        try {
            file.parentFile?.mkdirs()
            FileOutputStream(tmp).use { out ->
                out.write(sealed)
                out.fd.sync()
            }
            Files.move(tmp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
        } catch (e: IOException) {
            tmp.delete()
            throw fail("io", e)
        }
    }

    /** Called from Rust via JNI: static, `(Landroid/app/Activity;Ljava/lang/String;)Z`. `true` if removed. */
    @JvmStatic
    fun delete(activity: Activity, name: String): Boolean = synchronized(lock) {
        val file = blob(activity, name)
        File(file.path + ".tmp").delete()
        if (!file.exists()) return false
        if (!file.delete()) throw SecureStoreException("io: could not delete ${file.name}")
        true
    }

    /**
     * Called from Rust via JNI: static, `(Landroid/app/Activity;)I`. Where the
     * key lives: 0 software, 1 TEE, 2 StrongBox. Creates the key if needed.
     */
    @JvmStatic
    fun securityLevel(@Suppress("UNUSED_PARAMETER") activity: Activity): Int = synchronized(lock) {
        try {
            val key = key(activity, unlocked = false, create = true)!!
            val info = SecretKeyFactory.getInstance(key.algorithm, PROVIDER)
                .getKeySpec(key, KeyInfo::class.java) as KeyInfo
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                when (info.securityLevel) {
                    KeyProperties.SECURITY_LEVEL_SOFTWARE -> 0
                    KeyProperties.SECURITY_LEVEL_STRONGBOX -> 2
                    else -> 1
                }
            } else {
                @Suppress("DEPRECATION")
                if (info.isInsideSecureHardware) 1 else 0
            }
        } catch (e: GeneralSecurityException) {
            throw fail("keystore", e)
        } catch (e: ProviderException) {
            throw fail("keystore", e)
        }
    }

    private fun blob(activity: Activity, name: String): File =
        File(File(activity.noBackupFilesDir, DIR), name + SUFFIX)

    /** Checks the header and returns the IV length. */
    private fun parseHeader(data: ByteArray): Int {
        if (data.size < HEADER || !data.copyOfRange(0, 4).contentEquals(MAGIC)) {
            throw SecureStoreException("corrupt: not a secure store blob")
        }
        if (data[4] != VERSION) throw SecureStoreException("corrupt: unknown format version ${data[4]}")
        if (data[5].toInt() and FLAG_UNLOCKED.inv() != 0) {
            throw SecureStoreException("corrupt: unknown flags ${data[5]}")
        }
        val ivLen = data[6].toInt() and 0xff
        if (ivLen == 0 || data.size < HEADER + ivLen + TAG_BITS / 8) {
            throw SecureStoreException("corrupt: truncated")
        }
        return ivLen
    }

    /**
     * One of the store's keys; created on first use when [create]. The
     * [unlocked] key is refused while the device is locked, reported as
     * `locked` before Keystore is asked.
     */
    private fun key(activity: Activity, unlocked: Boolean, create: Boolean): SecretKey? {
        if (unlocked && activity.getSystemService(KeyguardManager::class.java).isDeviceLocked) {
            throw SecureStoreException("device-locked: the device is locked")
        }
        val alias = if (unlocked) UNLOCKED_ALIAS else ALIAS
        val store = try {
            KeyStore.getInstance(PROVIDER).apply { load(null) }
        } catch (e: IOException) {
            throw fail("keystore", e)
        }
        (store.getKey(alias, null) as SecretKey?)?.let { return it }
        if (!create) return null
        if (GeneratedPlugins.SECURE_STORE_STRONGBOX) {
            try {
                return generate(alias, unlocked, strongBox = true)
            } catch (e: StrongBoxUnavailableException) {
                Log.i(NativeLib.TAG, "secure store: no StrongBox, using the TEE")
            }
        }
        return generate(alias, unlocked, strongBox = false)
    }

    private fun generate(alias: String, unlocked: Boolean, strongBox: Boolean): SecretKey {
        val spec = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .setIsStrongBoxBacked(strongBox)
            .setUnlockedDeviceRequired(unlocked)
            .build()
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(spec)
        return generator.generateKey()
    }

    private fun fail(kind: String, cause: Throwable) =
        SecureStoreException("$kind: ${cause.javaClass.simpleName}: ${cause.message}", cause)
}
