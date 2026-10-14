// `harken-peer-kt`: harken's headless peer on the Kotlin client, speaking
// `harken-peer`'s JSON lines (harken/server/src/bin/harken-peer.rs), so the
// fleet runs Kotlin peers beside Rust ones (docs/plan-phones.md round 4).
// `installDist` lays out the launcher and every jar it needs; the flake's
// `packages.harken-peer-kt` is that directory with the JDK named.
plugins {
    kotlin("jvm")
    application
}

group = "dev.harken"
version = "0.1.0"

repositories {
    mavenCentral()
}

kotlin {
    jvmToolchain(21)
}

dependencies {
    implementation(project(":ark-client"))
}

application {
    mainClass.set("dev.harken.peer.HarkenPeerKt")
    applicationName = "harken-peer-kt"
    applicationDefaultJvmArgs = listOf("-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8")
}

// harken's module, as the domain crate writes it and the repository commits
// it: what the peer runs when `--module` names nothing else.
tasks.named<ProcessResources>("processResources") {
    from(rootProject.file("../harken/domain/harken.ark"))
}

// What a phone's launch costs, stage by stage, over a replica the client
// wrote (src/test/.../ColdOpen.kt). A measurement, not a check: it is run by
// hand, a fresh JVM per launch, with the classpath this task prints.
val coldOpenClasspath by tasks.registering {
    group = "verification"
    description = "Prints the classpath ColdOpen runs on."
    dependsOn(tasks.named("testClasses"))
    doLast { println("classpath=" + sourceSets["test"].runtimeClasspath.asPath) }
}
