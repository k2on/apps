// `ark-client`: a peer's shell around the runtime's client machine — the
// socket, the timer-driven pump, the files a replica survives a restart in,
// and the one call an app makes to author an intent. Android-safe: nothing
// from java.nio.file or java.net.http; the WebSocket is OkHttp's, which is
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

// The client's tests are a plain `main`, like the runtime's conformance
// runner, so that they need no test framework on the classpath.
val clientTests by tasks.registering(JavaExec::class) {
    group = "verification"
    description = "Runs ark-client's tests against the demo module."
    dependsOn(tasks.named("testClasses"))
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("dev.arkdb.client.ClientTests")
    val demo = providers.gradleProperty("arkDemoModule")
        .orElse(rootProject.file("../spec/vectors/module/demo.json").absolutePath)
    args(demo.get())
}

tasks.named("check") {
    dependsOn(clientTests)
}
