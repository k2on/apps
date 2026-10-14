// `ark-client`: a peer's shell around the runtime's client machine — the
// files a replica survives a restart in (`Storage`, in the records
// rust/ark-client writes), the peer that writes them and goes alone and
// back (`Peer`), and the WebSocket a transport opens. Android-safe at the
// app's minSdk 26: java.nio.file only where API 26 has it (a directory
// synced), nothing from java.net.http; the WebSocket is OkHttp's, which is
// the standard Android client and runs on the JVM unchanged.
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
    api(project(":ark-runtime"))
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
}

// The runtime's conformance runner — the trace reader and replayer, the
// JSON dialect — is what these tests drive the storage through, so its
// sources are compiled into this test source set too.
sourceSets["test"].kotlin.srcDir(project(":ark-runtime").file("src/test/kotlin"))

// The storage's tests are a plain `main`, like the runtime's conformance
// runner: the Rust-written fixture byte for byte, and every trace with the
// durable copy kept in these records.
val storageTests by tasks.registering(JavaExec::class) {
    group = "verification"
    description = "Holds the storage to a Rust peer's bytes and every trace to it."
    dependsOn(tasks.named("testClasses"))
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("dev.arkdb.client.StorageTests")
    jvmArgs("-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8")
    maxHeapSize = "1g"
    val vectors = providers.gradleProperty("arkVectors")
        .orElse(rootProject.file("../spec/vectors").absolutePath)
    val corpus = providers.gradleProperty("arkTraceCorpus")
        .orElse(providers.environmentVariable("ARK_TRACE_CORPUS"))
        .orElse("")
    args(vectors.get(), file("fixtures/rust-peer.json").absolutePath, corpus.get())
}

// The driver's half without a network: the link's backoff, the dialect, the
// sign-in's URLs, and views over harken's module.
val clientTests by tasks.registering(JavaExec::class) {
    group = "verification"
    description = "Holds the link, the dialect, the sign-in and the views."
    dependsOn(tasks.named("testClasses"))
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("dev.arkdb.client.ClientTests")
    jvmArgs("-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8")
    args(rootProject.file("../harken/domain/harken.ark").absolutePath)
}

// Launch views over harken's module: a file is the list the opened peer
// hydrates, corrected when stale, refused when bound elsewhere or torn.
val launchViewTests by tasks.registering(JavaExec::class) {
    group = "verification"
    description = "Holds launch views to the live views they stand in for."
    dependsOn(tasks.named("testClasses"))
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("dev.arkdb.client.LaunchViewTests")
    jvmArgs("-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8")
    args(rootProject.file("../harken/domain/harken.ark").absolutePath)
}

tasks.named("check") {
    dependsOn(storageTests, clientTests, launchViewTests)
}
