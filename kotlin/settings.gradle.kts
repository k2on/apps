pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

rootProject.name = "arkdb-kotlin"

// `ark-client` is a v3 peer over the runtime this round rewrites under it;
// it is left out of the build until round 3 brings it forward
// (docs/plan-phones.md), its sources kept as they were.
include("ark-runtime")
