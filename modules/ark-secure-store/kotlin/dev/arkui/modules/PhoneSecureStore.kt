// ark-secure-store on Android (modules/ark-secure-store/README.md): the `SecureStore` interface
// UniFFI writes, over the Android keystore.
//
// Each secret is encrypted with one AES-256-GCM key the keystore makes and keeps (it never leaves
// the keystore, and on a phone with one, the secure hardware), and the IV and ciphertext are kept,
// base64, in the app's private preferences file `ark-secure-store`, which the app's backup and
// data-extraction rules leave out (module.toml). The key's name is bound in as the associated data,
// so one entry can't be read as another's. Not Jetpack Security's EncryptedSharedPreferences: it is
// deprecated, and would be a Maven dependency for what is a cipher and a preferences file here.
package dev.arkui.modules

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.ark_secure_store.SecureStore
import uniffi.ark_secure_store.SecureStoreException

/** Secrets kept by this phone: encrypted by the keystore, kept in app-private preferences. */
class PhoneSecureStore(context: Context) : SecureStore {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    override fun get(key: String): String? {
        val kept = prefs.getString(key, null) ?: return null
        return try {
            val bytes = Base64.decode(kept, Base64.NO_WRAP)
            if (bytes.size <= IV) throw AEADBadTagException("too short")
            val c = Cipher.getInstance(CIPHER)
            c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, bytes, 0, IV))
            c.updateAAD(key.toByteArray(Charsets.UTF_8))
            String(c.doFinal(bytes, IV, bytes.size - IV), Charsets.UTF_8)
        } catch (x: AEADBadTagException) {
            // Kept under another key (the keystore's was lost: a restore, a wipe of the lock
            // screen): it can never be read again, so it is gone and the app asks afresh.
            prefs.edit().remove(key).commit()
            null
        } catch (x: IllegalArgumentException) {
            prefs.edit().remove(key).commit()
            null
        } catch (x: GeneralSecurityException) {
            throw SecureStoreException.Failed(why(x))
        }
    }

    override fun put(key: String, value: String) {
        val sealed =
            try {
                val c = Cipher.getInstance(CIPHER)
                // The keystore picks the IV: it refuses one given for encryption.
                c.init(Cipher.ENCRYPT_MODE, key())
                c.updateAAD(key.toByteArray(Charsets.UTF_8))
                val ct = c.doFinal(value.toByteArray(Charsets.UTF_8))
                c.iv + ct
            } catch (x: GeneralSecurityException) {
                throw SecureStoreException.Failed(why(x))
            }
        if (!prefs.edit().putString(key, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()) {
            throw SecureStoreException.Failed("this phone couldn't write it down")
        }
    }

    override fun remove(key: String) {
        if (!prefs.edit().remove(key).commit()) throw SecureStoreException.Failed("this phone couldn't forget it")
    }

    /** The store's one key, made the first time it is needed. */
    private fun key(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let {
            return it
        }
        val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        g.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build())
        return g.generateKey()
    }

    /** What went wrong, in the platform's words: the exception's kind, never anything it held. */
    private fun why(x: Exception): String = "the keystore: ${x.javaClass.simpleName}"

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "ark-secure-store"
        const val FILE = "ark-secure-store"
        const val CIPHER = "AES/GCM/NoPadding"
        const val IV = 12
        const val TAG_BITS = 128
    }
}
