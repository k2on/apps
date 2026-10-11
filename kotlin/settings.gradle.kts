pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

rootProject.name = "arkdb-kotlin"

// `ark-runtime`, the client's engine (dev.arkdb), and `ark-client`, the
// peer around it: its storage, alone and back, its transport.
include("ark-runtime")
include("ark-client")
