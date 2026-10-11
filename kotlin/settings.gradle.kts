pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

rootProject.name = "arkdb-kotlin"

// `ark-runtime`, the client's engine (dev.arkdb), and `ark-client`, the
// peer around it: its storage, alone and back, its link and sign-in; and
// `harken-peer`, harken's headless peer on it, which the fleet drives.
include("ark-runtime")
include("ark-client")
include("harken-peer")
