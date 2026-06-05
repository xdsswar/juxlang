plugins {
    // Lets Gradle auto-download the JDK toolchain (JDK 21) the IntelliJ
    // Platform build requires — no manual JDK install needed.
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "jux-intellij"
