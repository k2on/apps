// The plugins' kotlin-stdlib-jdk7/jdk8 at the compiler's version, the one
// the recorded graph (deps.json) holds the jars of; left alone, AGP's
// classpath settles on 1.9.20's, of which it holds only the POMs.
buildscript {
    dependencies {
        constraints {
            classpath("org.jetbrains.kotlin:kotlin-stdlib-jdk7:2.0.21")
            classpath("org.jetbrains.kotlin:kotlin-stdlib-jdk8:2.0.21")
        }
    }
}

plugins {
    id("com.android.application") version "8.7.3" apply false
    id("com.android.library") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
