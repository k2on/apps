// `ark-runtime`: the library generated Kotlin compiles against (see
// spec/GENERATED.md), held to spec/vectors by the conformance runner.
//
// Kotlin stdlib only. No java.nio.file, no JVM-only APIs beyond
// java.util, java.io and java.security.MessageDigest, so the same source
// compiles for Android.
plugins {
    kotlin("jvm")
}

group = "dev.arkdb"
version = "0.1.0"

repositories {
    mavenCentral()
}

kotlin {
    jvmToolchain(21)
}

dependencies {
    // Nothing. The runtime depends on the Kotlin stdlib alone.
}

// The conformance runner is a plain `main` rather than a JUnit suite, so
// that it needs no test framework on the classpath and so that the
// kotlinc fallback (`build.sh`) runs the very same code.
val conformance by tasks.registering(JavaExec::class) {
    group = "verification"
    description = "Runs the conformance vectors under spec/vectors."
    dependsOn(tasks.named("testClasses"))
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("dev.arkdb.conformance.Conformance")
    val vectors = providers.gradleProperty("arkVectors")
        .orElse(rootProject.file("../spec/vectors").absolutePath)
    args(vectors.get())
}

tasks.named("check") {
    dependsOn(conformance)
}
