// Jux IntelliJ Platform plugin (JUX-INTELLIJ-PLUGIN-ADDENDUM.md §I.2).
// Built with the IntelliJ Platform Gradle Plugin 2.x — Kotlin DSL only.
//
// Toolchain: Gradle 9.x + JDK 21. IntelliJ IDEA 2026.1 runs on JBR 21, and the
// IntelliJ Platform Gradle Plugin builds plugins against the IDE's JDK — it
// rejects a JDK 25 toolchain. JDK 21 also guarantees the plugin loads in the
// 2026.1.3 IDE with no class-version crash. The foojay resolver in
// settings.gradle.kts auto-downloads JDK 21, so no manual JDK install is needed.
plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.2.0"
    id("org.jetbrains.intellij.platform") version "2.16.0"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

repositories {
    mavenCentral()
    // IntelliJ Platform artifacts come from JetBrains' repositories.
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // Unified IntelliJ IDEA distribution as the compile/run target.
        // (The separate `ideaIC` Community artifact was discontinued at
        // 2025.3; `intellijIdea(...)` is the current entry point.)
        intellijIdea(providers.gradleProperty("platformVersion").get())
    }
}

intellijPlatform {
    pluginConfiguration {
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            // No untilBuild cap: track the latest stable on each push.
            untilBuild = provider { null }
        }
    }
}

// One JDK 21 toolchain drives both Java and Kotlin (and their bytecode target),
// matching the IDE's JBR. foojay auto-provisions it if JDK 21 isn't installed.
kotlin {
    jvmToolchain(21)
}
