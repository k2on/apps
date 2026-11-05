// What this phone is called in somebody else's device picker: a name a person
// would call it, never a build code (Device.kt, `DeviceName`).
package dev.harken.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceNameTests {
    /** The name its owner gave it, or its Bluetooth name, comes first. */
    @Test
    fun aGivenNameIsTheName() {
        assertEquals("Ana's Pixel", DeviceName.of(listOf("Ana's Pixel", "Pixel 7"), "Google", "Pixel 7"))
        assertEquals("the Bluetooth name, with none given", "Ana's Phone", DeviceName.of(listOf(null, "Ana's Phone"), "samsung", "SM-S921B"))
    }

    /** A name that is a build code is passed over for the maker and the model. */
    @Test
    fun aBuildCodeIsNoName() {
        assertEquals("Pixel 7", DeviceName.of(listOf("panther", null), "Google", "Pixel 7"))
        assertEquals("Samsung SM-S921B", DeviceName.of(listOf("  "), "samsung", "SM-S921B"))
        assertEquals("the model alone where it says the maker", "OnePlus 12", DeviceName.of(emptyList(), "OnePlus", "OnePlus 12"))
    }

    /** The emulator the screenshots are taken on is "Android Emulator", not its build. */
    @Test
    fun anEmulatorSaysSo() {
        val build = listOf("google/sdk_gphone64_x86_64/emu64xa:14/UE1A.230829.036/10808617:userdebug/dev-keys", "sdk_gphone64_x86_64", "ranchu")
        assertEquals("Android Emulator", DeviceName.of(listOf("sdk_gphone64_x86_64", null), "Google", "sdk_gphone64_x86_64", build))
        assertEquals("by its model alone too", "Android Emulator", DeviceName.of(emptyList(), "unknown", "sdk_gphone64_arm64"))
    }

    /** What a build code looks like: underscores, an emulator's prefix, one lower-case word. */
    @Test
    fun rawCodes() {
        assertTrue(DeviceName.raw("sdk_gphone64_x86_64"))
        assertTrue(DeviceName.raw("generic x86"))
        assertTrue(DeviceName.raw("panther"))
        assertFalse(DeviceName.raw("Pixel 7"))
        assertFalse(DeviceName.raw("Ana's phone"))
    }

    /** Nothing said at all is still something to call it. */
    @Test
    fun nothingSaidIsAPhone() {
        assertEquals("Phone", DeviceName.of(emptyList(), null, null))
    }
}
