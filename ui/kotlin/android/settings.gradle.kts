// ArkUI's Kotlin as Android libraries (ui/kotlin/README.md): `:shots`
// (ui/kotlin/shots, every app's screenshot mode) and `:arkui` (ui/kotlin/src,
// the look, over `:shots`). nix builds each once as a derivation of its own
// (flake.nix, `arkuiKotlin`: `packages.arkui-kotlin-shots`,
// `packages.arkui-kotlin`), and every app links the jars instead of compiling
// the sources again; a laptop's app build still compiles them from source.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "arkui-android"

include(":shots", ":arkui")
