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

        // LSP4IJ: compile-time API for the Community-edition fallback client
        // (META-INF/lsp4ij.xml → dev.jux.intellij.lsp4ij). Optional at
        // runtime via <depends optional="true">. Pinned: LSP4IJ is pre-1.0,
        // bump deliberately and re-test. NEVER add an org.eclipse.lsp4j
        // dependency here — LSP4J loads from LSP4IJ's classloader, and a
        // second copy causes ClassCastExceptions.
        plugin("com.redhat.devtools.lsp4ij:0.19.4")

        // Headless platform test fixtures (ParsingTestCase et al.).
        testFramework(org.jetbrains.intellij.platform.gradle.TestFrameworkType.Platform)
    }
    testImplementation("junit:junit:4.13.2")
}

// Sandbox run on the last Community line (IC discontinued at 2025.3) with
// LSP4IJ installed — verifies the fallback client end-to-end:
//   .\gradlew.bat runIdeCommunity
val runIdeCommunity by intellijPlatformTesting.runIde.registering {
    type = org.jetbrains.intellij.platform.gradle.IntelliJPlatformType.IntellijIdeaCommunity
    version = "2025.1.3"
    plugins {
        plugin("com.redhat.devtools.lsp4ij:0.19.4")
    }
}

tasks.test {
    useJUnit()
}

intellijPlatform {
    pluginConfiguration {
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            // No untilBuild cap: track the latest stable on each push.
            untilBuild = provider { null }
        }
    }
    pluginVerification {
        // Fail ONLY on real compatibility problems (unresolved classes/
        // methods/fields → runtime linkage errors). Internal-API usage stays a
        // report-only signal: on 2024.2 the verifier flags Kotlin
        // interface-bridge artifacts (`ToolWindowFactory.getAnchor/getIcon/
        // manage` "overridden" by our factory without any such source
        // override) that cannot be removed by editing source — failing on
        // them would make every verify run red regardless of our code.
        failureLevel = listOf(
            org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask.FailureLevel.COMPATIBILITY_PROBLEMS,
        )
    }
}

// One JDK 21 toolchain drives both Java and Kotlin (and their bytecode target),
// matching the IDE's JBR. foojay auto-provisions it if JDK 21 isn't installed.
kotlin {
    jvmToolchain(21)
}

// ---------------------------------------------------------------------------
// Token-layer single-sourcing (Phase 0 of the PSI work).
//
// `grammar/jux-tokens.json` is emitted from the canonical Rust lexer
// (`juxc-lex` `grammar_spec`). This task generates the plugin's token registry
// (`JuxTokenTypes`) and keyword/primitive sets (`JuxKeywords`) from it, so the
// IDE's token alphabet can never drift from the compiler's. Regenerate the JSON
// with `JUX_BLESS=1 cargo test -p juxc-lex grammar_spec`.
// ---------------------------------------------------------------------------
val juxTokensJson = layout.projectDirectory.file("grammar/jux-tokens.json")
val generatedTokensDir = layout.buildDirectory.dir("generated/sources/juxTokens/kotlin/main")

val generateJuxTokens by tasks.registering {
    description = "Generates JuxTokenTypes/JuxKeywords from grammar/jux-tokens.json."
    val input = juxTokensJson
    val outDir = generatedTokensDir
    inputs.file(input)
    outputs.dir(outDir)

    doLast {
        @Suppress("UNCHECKED_CAST")
        val spec = groovy.json.JsonSlurper().parse(input.asFile) as Map<String, Any>

        fun tokens(key: String): List<Map<String, Any?>> =
            (spec[key] as List<*>).map {
                @Suppress("UNCHECKED_CAST") (it as Map<String, Any?>)
            }
        fun strings(key: String): List<String> = (spec[key] as List<*>).map { it.toString() }
        fun names(key: String): List<String> = tokens(key).map { it["name"].toString() }

        val keywords = tokens("keywords")
        val literals = names("literals")
        val punctuation = names("punctuation")
        val operators = names("operators")
        val comments = names("comments")
        val primitives = strings("primitives")
        val constants = strings("constants")
        val keywordNames = keywords.map { it["name"].toString() }

        val lang = "JuxLanguage"
        fun decl(name: String) = "    val $name = IElementType(\"$name\", $lang)"
        fun tokenSet(name: String, members: List<String>) =
            "    val $name: TokenSet = TokenSet.create(${members.joinToString(", ")})"

        val sb = StringBuilder()
        sb.appendLine("// GENERATED — do not edit. Source: grammar/jux-tokens.json (juxc-lex grammar_spec).")
        sb.appendLine("// Regenerate the JSON with: JUX_BLESS=1 cargo test -p juxc-lex grammar_spec")
        sb.appendLine("package dev.jux.intellij.highlight")
        sb.appendLine()
        sb.appendLine("import com.intellij.psi.tree.IElementType")
        sb.appendLine("import com.intellij.psi.tree.TokenSet")
        sb.appendLine("import dev.jux.intellij.JuxLanguage")
        sb.appendLine()
        sb.appendLine("/**")
        sb.appendLine(" * The Jux token alphabet — one [IElementType] per lexer token, generated from")
        sb.appendLine(" * the compiler's canonical token list. Grouping [TokenSet]s drive the syntax")
        sb.appendLine(" * highlighter, brace matcher, and parser.")
        sb.appendLine(" */")
        sb.appendLine("object JuxTokenTypes {")
        sb.appendLine(decl("IDENTIFIER"))
        sb.appendLine()
        sb.appendLine("    // Keywords")
        keywordNames.forEach { sb.appendLine(decl(it)) }
        sb.appendLine()
        sb.appendLine("    // Literals")
        literals.forEach { sb.appendLine(decl(it)) }
        sb.appendLine()
        sb.appendLine("    // Punctuation")
        punctuation.forEach { sb.appendLine(decl(it)) }
        sb.appendLine()
        sb.appendLine("    // Operators")
        operators.forEach { sb.appendLine(decl(it)) }
        sb.appendLine()
        sb.appendLine("    // Comments")
        comments.forEach { sb.appendLine(decl(it)) }
        sb.appendLine()
        sb.appendLine(tokenSet("KEYWORDS", keywordNames))
        sb.appendLine(tokenSet("LITERALS", literals))
        sb.appendLine(tokenSet("PUNCTUATION", punctuation))
        sb.appendLine(tokenSet("OPERATORS", operators))
        sb.appendLine(tokenSet("COMMENTS", comments))
        // Stable sub-groups the editor needs by structural name.
        val stringLits = literals.filter { it.endsWith("STRING_LITERAL") || it == "CHAR_LITERAL" }
        sb.appendLine(tokenSet("STRING_LITERALS", stringLits))
        sb.appendLine(tokenSet("BRACES", listOf("LBRACE", "RBRACE")))
        sb.appendLine(tokenSet("BRACKETS", listOf("LBRACKET", "RBRACKET")))
        sb.appendLine(tokenSet("PARENS", listOf("LPAREN", "RPAREN")))
        sb.appendLine()
        val mapEntries = keywords.joinToString(",\n") {
            "        \"${it["spelling"]}\" to ${it["name"]}"
        }
        sb.appendLine("    private val KEYWORD_BY_TEXT: Map<String, IElementType> = mapOf(")
        sb.appendLine(mapEntries)
        sb.appendLine("    )")
        sb.appendLine()
        sb.appendLine("    /** The keyword token for [text], or null if [text] is not a reserved word. */")
        sb.appendLine("    fun keywordType(text: String): IElementType? = KEYWORD_BY_TEXT[text]")
        sb.appendLine("}")

        val pkgDir = outDir.get().dir("dev/jux/intellij/highlight").asFile
        pkgDir.mkdirs()
        pkgDir.resolve("JuxTokenTypes.kt").writeText(sb.toString())

        val kw = StringBuilder()
        kw.appendLine("// GENERATED — do not edit. Source: grammar/jux-tokens.json (juxc-lex grammar_spec).")
        kw.appendLine("// Regenerate the JSON with: JUX_BLESS=1 cargo test -p juxc-lex grammar_spec")
        kw.appendLine("package dev.jux.intellij.highlight")
        kw.appendLine()
        kw.appendLine("/**")
        kw.appendLine(" * Word sets shared with the compiler: reserved [KEYWORDS], built-in")
        kw.appendLine(" * [PRIMITIVES] type names, and literal [CONSTANTS].")
        kw.appendLine(" */")
        kw.appendLine("object JuxKeywords {")
        fun strSet(name: String, values: List<String>) =
            "    val $name: Set<String> = setOf(${values.joinToString(", ") { "\"$it\"" }})"
        kw.appendLine(strSet("KEYWORDS", keywords.map { it["spelling"].toString() }))
        kw.appendLine(strSet("PRIMITIVES", primitives))
        kw.appendLine(strSet("CONSTANTS", constants))
        kw.appendLine("}")
        pkgDir.resolve("JuxKeywords.kt").writeText(kw.toString())
    }
}

kotlin.sourceSets.named("main") {
    kotlin.srcDir(generateJuxTokens)
}
