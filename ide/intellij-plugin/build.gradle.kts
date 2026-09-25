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

        // Spellchecker: a platform CONTENT MODULE (lib/intellij.spellchecker.jar),
        // not a bundled plugin -- it declares the alias
        // `com.intellij.modules.spellchecker`, which is what the descriptor
        // depends on, optionally (META-INF/spellchecker.xml). Compile-time API only.
        bundledModule("intellij.spellchecker")

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

    // Two test classes read the repository's `examples/` corpus at runtime
    // (JuxParsingTest, JuxCorpusHighlightingTest) rather than from the module's
    // own resources, so Gradle cannot see the dependency and happily replays a
    // cached PASS after the corpus changes. That makes both of them look like
    // guards while guarding nothing: an example added to reproduce an editor
    // bug would never actually be run through the annotator.
    //
    // Declaring the corpus as an input fixes the caching AND the up-to-date
    // check. `PathSensitivity.RELATIVE` so a checkout at a different path still
    // hits the cache.
    inputs.dir(layout.projectDirectory.dir("../../examples"))
        .withPathSensitivity(PathSensitivity.RELATIVE)
        .withPropertyName("juxExampleCorpus")
}

intellijPlatform {
    pluginConfiguration {
        // Shown in Settings | Plugins after an update. There is no CHANGELOG.md
        // and no org.jetbrains.changelog plugin, so this block is the whole
        // mechanism -- keep it to what a user would notice.
        changeNotes = """
            <h3>0.1.7</h3>
            <p>The editor follows the compiler again. 0.1.6 flagged code that compiles; this fixes the cause rather than the symptoms.</p>
            <ul>
              <li><b>A name is resolved in the file that wrote it.</b> A bare type name no longer finds a same-named class in an unrelated file, so declaring your own <b>Iterable</b>, <b>T</b>, <b>String</b> or <b>Vec</b> stops making other files look broken. The standard library resolves its own names against itself, which is what lets you take those names at all.</li>
              <li><b>An aliased or fully-qualified type is never shadowed</b>: <b>import rust.std.Vec as RVec;</b> beside your own <b>class Vec</b> resolves to the library type, as it does in the compiler.</li>
              <li><b>The positional subclass pattern is understood</b>: <b>case Circle(r)</b> over a sealed class binds <b>r</b> to the subclass field it names, so a guard that reads it is typed.</li>
              <li><b>Members through a wildcard bound resolve</b>: a wildcard argument used to be dropped entirely, so <b>Vec&lt;? extends Animal&gt;</b> held nothing and every member through it was unresolved.</li>
              <li><b>A src/bin entry file needs no package declaration</b>, matching the multi-binary project rule. A helper beside it still does.</li>
            </ul>
            <h3>0.1.6</h3>
            <p>A rebuild against a compiler that changed a great deal underneath. The editor itself is unchanged since 0.1.5; this release exists so the plugin you install matches the toolchain you build with.</p>
            <ul>
              <li><b>Built against the compiler that now lets you name a class anything</b>: <b>T</b>, <b>String</b>, <b>Vec</b>, <b>Exception</b>, <b>Err</b> and <b>Result</b> are ordinary type names, and the library type is still reachable beside one through an <b>import ... as</b> alias or its full name.</li>
              <li><b>Built against the sealed-class change</b>: a sealed hierarchy is a reference hierarchy now, so a subclass goes into a <b>Vec&lt;Base&gt;</b> and a mutating override works through a base-typed name.</li>
              <li><b>Built against the multi-binary project rule</b>: a package with <b>[lib]</b> and several <b>[[bin]]</b> targets, with entries under <b>src/bin/</b>, is a valid project.</li>
              <li>Known gap, being worked on now: the editor does not yet know the new positional subclass pattern (<b>case Circle(r)</b> over a sealed class), nor the members a wildcard over a class bound now reaches. The compiler accepts both; the editor may still mark them.</li>
            </ul>
            <h3>0.1.5</h3>
            <p>The editor catches up with two compiler waves: <b>ref</b> bindings, and the standard library's slice and string methods.</p>
            <ul>
              <li><b>ref bindings are checked as the compiler checks them</b>: a <b>ref</b> return type, <b>ref ref T</b> and a <b>ref</b> generic argument are reported with the compiler's own wording, and a <b>ref</b> on a class, interface, array or collection is a warning with a fix that removes the keyword. A <b>ref</b> on a value type, which is what the feature is for, is never touched.</li>
              <li><b>A ref binding captured by a Worker.spawn closure</b> is reported: the cell is task-local and cannot cross the thread boundary.</li>
              <li><b>The library's slice and string methods complete and resolve</b>: <b>sort</b>, <b>sort_by</b>, <b>to_vec</b>, <b>join</b>, <b>binary_search</b> and the rest on a <b>Vec</b>, <b>to_uppercase</b> and friends on a <b>String</b>. An array offers its own members instead of an empty popup.</li>
              <li><b>A foreign method's bound is checked at the call</b>: <b>people.sort()</b> on a <b>Vec&lt;Person&gt;</b> says <b>Person</b> declares no <b>operator&lt;=&gt;</b>, instead of letting the Rust compiler say it.</li>
              <li><b>Every built-in type completes</b>, read from the compiler's own alphabet, both <b>String</b> and <b>string</b> included, everywhere a type may be written.</li>
              <li><b>Better completion order</b>: this file's types first, then this file's package, the built-in types, what the file already imports, the prelude, and last the names that would need an <b>import</b>. A prelude name such as <b>Vec</b> no longer writes one.</li>
            </ul>
            <h3>0.1.4</h3>
            <p>Imports stop flickering "unresolved": the generated crate stubs parse, keyword paths are accepted, and a stub is never missing while it regenerates.</p>
            <ul>
              <li><b>Generated .jux.d stubs parse in full</b>: a member named with a reserved word (<b>NonNull.new</b>, <b>Stdio.null</b>, <b>Shader.new</b>) is accepted exactly as the compiler accepts it, so the bundled standard library stub no longer stops being indexed at its first <b>new</b>.</li>
              <li><b>Keywords in import and package paths</b>: <b>import rust.x.record;</b> and <b>package demo.type;</b> are names, not errors, and so is a keyword-spelled import alias.</li>
              <li><b>A keyword used as a member</b> (<b>window.default()</b>) is colored as the member it is rather than as a keyword.</li>
              <li><b>Workspace members see their siblings</b>: always-on diagnostics now check from the <b>[workspace]</b> root, as the language server does, so a sibling package's types resolve.</li>
              <li><b>A regenerating crate stub stays readable</b>: the old declarations are kept until the new stub lands, instead of the file being removed for the minutes the generation takes.</li>
              <li><b>No import for a type you already see</b>: completion, the Import type fix and add-on-the-fly never write an <b>import</b> for a type declared in this file or in this file's package, including a file with no <b>package</b> line, whose package comes from where it sits.</li>
            </ul>
            <h3>0.1.3</h3>
            <p>Function types, assigned lambdas, comparators and variance, and sturdier completion.</p>
            <ul>
              <li><b>Function types</b> are real types: a call through a function-typed local has the result type, and completion ranks function values by variance (parameters contravariant, result covariant).</li>
              <li><b>Assigned lambdas</b> (<b>p = (a, b) -&gt; ...</b>, <b>this.f = ...</b>) take their parameter types from the interface they are assigned to.</li>
              <li><b>Comparators</b>: a lambda passed where an <b>Ordering</b> is expected may return an integer; anything else is reported as the compiler does.</li>
              <li><b>Variance</b>: a function value assigned the wrong way round is reported with the compiler's message.</li>
              <li>A null check outside a lambda no longer counts inside it, as in the compiler.</li>
              <li>Fixed: completing again in a reloaded file could fail on stale PSI.</li>
            </ul>
            <h3>0.1.2</h3>
            <p>The build system comes to the IDE: profiles, examples, workspaces, doc examples and framed errors.</p>
            <ul>
              <li><b>Run configurations</b> pick a <b>--profile</b> (built-ins plus your <b>[profile.*]</b>) and an <b>--example</b> from <b>examples/</b>, or build all examples; a new <b>Doc examples</b> mode runs <b>jux test --doc</b>.</li>
              <li><b>Doc examples in the test tree</b>: each <b>```jux</b> example is a green or red node that opens the item it documents; a failure shows its reason.</li>
              <li><b>jux.toml</b>: completion and checks for <b>[workspace]</b> (<b>members</b> patterns, <b>exclude</b>, <b>default-members</b>) and <b>key.workspace = true</b>, with Ctrl+B to the root's declaration.</li>
              <li><b>Tools | Jux</b>: <b>new</b> (binary, library, workspace), <b>init</b>, <b>add</b> (path, git, tag, features), <b>remove</b>, <b>tree</b>, <b>doc</b>, <b>doc --open</b>, <b>clean</b>.</li>
              <li><b>Doc comments</b>: tags colored and completed, <b>```jux</b> examples highlighted as Jux with their <b>ignore</b> / <b>no_run</b> flags, and Quick Documentation rendered like <b>jux doc</b> (Markdown, highlighted examples, <b>@deprecated</b>).</li>
              <li><b>Framed, colored compile errors</b> in run and test consoles, and Alt+Enter <b>Explain error EXXXX</b> for any Jux diagnostic.</li>
              <li><b>New Project</b>: binary, library or workspace, made by <b>jux new</b> when the toolchain is installed.</li>
            </ul>

            <h3>0.1.1</h3>
            <p>The editor catches up with the newest Jux: the standard library, <b>never</b>, ranges as values, and the layout and ABI features.</p>
            <ul>
              <li><b>The standard library ships with the plugin</b>: <b>Option</b>, <b>Result</b>, the iterator combinators, <b>Mutex</b> and the exceptions complete, navigate (Ctrl+B) and show Quick Documentation with their real signatures.</li>
              <li><b>Lambda parameters know their type</b>: <b>opt.map((p) -&gt; p.</b> completes the element's members, and a lambda passed for a single-method interface gets that method's parameter types.</li>
              <li><b>Ranges</b> type as values (<b>start</b>, <b>end</b>, <b>endInclusive</b>, <b>step</b>), and a for-each over a map binds a <b>(K, V)</b> entry read as <b>e.0</b> / <b>e.1</b>.</li>
              <li><b>never</b>: a call to a never function ends its path, so no false "Missing return"; the never rules themselves are checked as you type.</li>
              <li><b>Layout and ABI</b> checks for <b>@align</b>, <b>array as T*</b>, <b>transmute</b> and free operators; a <b>// SAFETY:</b> comment check on unsafe blocks, with a fix.</li>
              <li><b>Java library calls</b> (<b>Math.abs</b>, <b>Integer.parseInt</b>, <b>Objects.equals</b>, <b>String.valueOf</b>) are reported with a one-click Jux rewrite.</li>
            </ul>

            <h3>0.1.0</h3>
            <p>More of IntelliJ's Java intelligence, in Jux's own terms.</p>
            <ul>
              <li><b>New inspections</b>: <b>Field may be 'final'</b>, <b>Variable is assigned but never read</b>, a <b>null check on a non-nullable value</b> (always true or false), and a <b>nullable value used without a check</b>, each with a fix. Off by default, as in Java: <b>Local variable may be 'final'</b> and <b>Explicit type can be replaced with 'var'</b>.</li>
              <li><b>Extract Interface</b> and <b>Extract Superclass</b> join Pull Up, Push Down and Change Signature in the Refactor menu.</li>
              <li><b>Completion</b>: right after <b>case</b>, only the enum constants or sealed subtypes no arm names yet; <b>chain completion</b> in smart completion (<b>order.customer.home</b> for an <b>Address</b> slot); <b>.switch</b> writes every arm for an enum or sealed value; <b>.yield</b>.</li>
              <li><b>Generate</b> (Alt+Insert): <b>operator&lt;=&gt;</b>, and <b>Properties</b> with get/set accessors over fields.</li>
              <li><b>Method-chain type hints</b>: in a chain written one call per line, the type each line produces.</li>
            </ul>

            <h3>0.0.9</h3>
            <p>Java's navigation and inspections, and the newest Jux syntax understood, not just parsed.</p>
            <ul>
              <li><b>Operators are symbols</b>: Ctrl+B on the <b>*</b> of <b>v * 2.0</b> goes to the <b>operator*</b> the compiler calls, chosen by operand type; the expression has that operator's type; Find Usages on an operator lists its uses. Interface operators get implement gutters.</li>
              <li><b>Generators</b>: <b>yield</b> completes only in a function returning <b>Iterator&lt;T&gt;</b> or <b>Stream&lt;T&gt;</b>, and the compiler's generator rules show as you type, with fixes.</li>
              <li><b>Implement / override completion</b>: in a class body, type the start of an inherited method's name and accept it to write the whole member.</li>
              <li><b>Create missing 'case' branches</b> for a switch over an enum or a sealed type; <b>'if' can be simplified</b>; <b>indexed loop can be a for-each</b>.</li>
              <li><b>Type Info</b> (Ctrl+Shift+P), <b>Type Declaration</b> (Ctrl+Shift+B), <b>Go to Test</b> / <b>Create Test</b>, and File Structure toggles for fields, properties, non-public and inherited members.</li>
              <li><b>Folding</b> with Java's regions, <b>Wrapping and Braces</b> and <b>Blank Lines</b> code-style tabs, and Quick Documentation laid out like Java's.</li>
              <li>Pattern binders, labels, method references and smart casts navigate and rename.</li>
            </ul>

            <h3>0.0.8</h3>
            <p>Broken code while you type: one clear message at the mistake, and a fix on Alt+Enter.</p>
            <ul>
              <li><b>Syntax errors name the mistake</b>: <b>'else' without 'if'</b>, <b>'catch' without 'try'</b>, <b>Unexpected ')'</b>, <b>Expression expected</b>, <b>',' or ')' expected</b>, and <b>'->' expected</b> on a Java-style <b>case 1:</b>.</li>
              <li><b>One mistake, one error</b>: a missing <b>}</b> no longer turns every following member red, and an unclosed <b>&lt;</b> or <b>[</b> stops at its own statement.</li>
              <li><b>Alt+Enter on a syntax error</b> inserts the missing <b>;</b> <b>)</b> <b>]</b> <b>}</b> <b>&gt;</b> or <b>,</b> after the last token, removes a stray token, or turns <b>case 1:</b> into <b>case 1 -&gt;</b>.</li>
              <li><b>Hints for common slips</b>, each with a fix: <b>if (x = 5)</b>, a lambda written with <b>=&gt;</b>, <b>'abc'</b> as a char, an unclosed string or comment, and <b>let</b>, <b>elif</b> or <b>function</b> from other languages.</li>
            </ul>

            <h3>0.0.7</h3>
            <p>A semantic engine: the editor now understands Jux the way IntelliJ understands Java.</p>
            <ul>
              <li><b>Completion after a dot follows real types</b>: method-call chains, <b>var</b> locals typed by a call, a bounded type parameter's members, inherited members, generics carried through (<b>Box&lt;Truck&gt;.get()</b> reads <b>Truck</b>), and the standard library and bound crates.</li>
              <li><b>Go to Declaration, Find Usages and Rename work across files and packages</b>, through chains, generics, imports and static calls. Renaming a class rewrites its imports.</li>
              <li><b>Auto-import like Java</b>: an unimported class is red with <b>Import 'pkg.Type'</b> on Alt+Enter, the "pkg.Type? Alt+Enter" hint appears by itself, and "Add unambiguous imports on the fly" is honoured. Never a duplicate import, a self-import, or an import from the same package.</li>
              <li><b>new Truck</b> completes as <b>new Truck(&lt;caret&gt;)</b> and imports the class.</li>
              <li><b>/**</b> + Enter writes <b>@param</b>, <b>@return</b> and <b>@throws</b> from the signature.</li>
              <li>With <b>juxc-lsp</b> running, the plugin keeps completion and navigation (the language server's generic versions are switched off, so nothing is shown twice); the server still supplies compile errors and hover.</li>
            </ul>

            <h3>0.0.6</h3>
            <p>Unsafe and C interop code reads the way the compiler reads it.</p>
            <ul>
              <li>A cast whose operand starts with <b>&amp;</b>, <b>*</b> or <b>-</b> parses: <b>(void*) &amp;n</b>, <b>(int) *p</b>, <b>(long) -n</b>. A bare <b>(count) * 2</b> is still a product.</li>
              <li><b>x in xs</b> and <b>out this.field</b> no longer show a false syntax error.</li>
              <li>Generated Rust binding stubs with borrowed parameters (<b>&amp;T</b>, <b>&amp;mut T</b>) parse, so Parameter Info shows the right names.</li>
              <li>Foreign functions from a <b>native</b> block are completed inside an <b>unsafe</b> function, not only inside an <b>unsafe { }</b> block.</li>
              <li>Diagnostics follow the new numeric rules of the compiler: a signed and unsigned integer with no common type, and constant overflow, are reported in the editor.</li>
            </ul>

            <h3>0.0.3</h3>
            <p>Language sync.</p>
            <ul>
              <li><b>string</b> is now recognised as a primitive type name, the same type as <b>String</b> (C#-style). Highlighting, completion and the type checker all treat the two spellings as one.</li>
            </ul>

            <h3>0.0.2</h3>
            <p>The everyday editor actions, and a real Refactor menu.</p>
            <ul>
              <li><b>Parameter Info</b> (Ctrl+P) for calls and constructors, every overload listed.</li>
              <li><b>Surround With</b> (Ctrl+Alt+T): if / while / for / try-catch / unsafe, and (expr) / !(expr).</li>
              <li><b>Complete Current Statement</b> (Ctrl+Shift+Enter): closes brackets, adds the missing semicolon, opens a body.</li>
              <li><b>Move Statement Up/Down</b> (Ctrl+Shift+Up/Down), which will not lift a statement out of its block.</li>
              <li><b>Type Hierarchy</b> (Ctrl+H): supertypes, subtypes, or both.</li>
              <li><b>Breadcrumbs</b> under the editor, and <b>spellchecking</b> of comments, plain strings and declaration names.</li>
              <li><b>Extract Variable</b> (Ctrl+Alt+V), <b>Introduce Constant</b> (Ctrl+Alt+C), <b>Inline Variable</b> (Ctrl+Alt+N) and <b>Safe Delete</b>.</li>
            </ul>
        """.trimIndent()

        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            // No untilBuild cap: track the latest stable on each push.
            untilBuild = provider { null }
        }
    }
    // Marketplace release (§I.9). Nothing secret lives in the repository: the
    // token and the signing material come from the environment of whoever
    // publishes, and `publishPlugin` fails with a clear message without them.
    // A version with a pre-release suffix (`0.4.0-rc.1`) goes to the `beta`
    // channel; a plain version goes to `default`.
    signing {
        certificateChain = providers.environmentVariable("CERTIFICATE_CHAIN")
        privateKey = providers.environmentVariable("PRIVATE_KEY")
        password = providers.environmentVariable("PRIVATE_KEY_PASSWORD")
    }
    publishing {
        token = providers.environmentVariable("PUBLISH_TOKEN")
        channels = providers.gradleProperty("pluginVersion").map { v -> listOf(if ('-' in v) "beta" else "default") }
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
        // The IDEs `verifyPlugin` checks against; without a selection the task
        // stops with "No IDE selected for verification".
        ides {
            recommended()
        }
    }
}

// ---------------------------------------------------------------------------
// Optional: ship a `juxc-lsp` binary inside the plugin (§I.6, §I.9).
//
// Off by default. Pass `-PjuxBundleLsp=<path to juxc-lsp[.exe]>` (or set
// `JUX_BUNDLE_LSP`) and `prepareSandbox`, and so `buildPlugin`, copy it into
// the plugin's `bin/` directory. `JuxToolchain` looks there last, after the
// configured toolchain, `$JUX_HOME`, `PATH` and the usual install locations,
// so an installed toolchain always wins. The binary is native code: a bundled
// build is a build for one platform.
// ---------------------------------------------------------------------------
val bundledLsp = providers.gradleProperty("juxBundleLsp").orElse(providers.environmentVariable("JUX_BUNDLE_LSP"))

tasks.named<org.jetbrains.intellij.platform.gradle.tasks.PrepareSandboxTask>("prepareSandbox") {
    if (bundledLsp.isPresent) {
        val binary = file(bundledLsp.get())
        require(binary.isFile) { "juxBundleLsp: no file at $binary" }
        from(binary) {
            into(pluginName.map { "$it/bin" })
            // Whatever the input is called, the plugin looks for `juxc-lsp[.exe]`.
            rename { if (it.endsWith(".exe")) "juxc-lsp.exe" else "juxc-lsp" }
        }
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
        kw.appendLine()
        kw.appendLine("    /**")
        kw.appendLine("     * Names bound with no declaration and no `import`: the prelude types")
        kw.appendLine("     * (`Vec`, `HashMap`, the exception hierarchy) and the language")
        kw.appendLine("     * intrinsics (`print`, `spawn`, `block_on`, `Worker`).")
        kw.appendLine("     *")
        kw.appendLine("     * Generated from the same list the compiler's resolver seeds itself")
        kw.appendLine("     * with, so a name added to the language is known here in the same")
        kw.appendLine("     * commit. The hand-written Kotlin copy this replaces had drifted: it")
        kw.appendLine("     * still believed `Vec` required an `import` long after it became")
        kw.appendLine("     * prelude, and the unresolved-reference inspection painted \"cannot")
        kw.appendLine("     * resolve type\" over 20 examples that compile.")
        kw.appendLine("     */")
        kw.appendLine(strSet("BUILTINS", strings("builtins")))
        kw.appendLine()
        kw.appendLine("    /**")
        kw.appendLine("     * The annotations the compiler HONORS, without the `@`, spelled as")
        kw.appendLine("     * the docs and the example corpus write them (names are matched")
        kw.appendLine("     * case-insensitively, so these are the offer spellings).")
        kw.appendLine("     *")
        kw.appendLine("     * Generated from the compiler's own list, which a drift test there")
        kw.appendLine("     * pins against the call sites that act on each name — so an")
        kw.appendLine("     * annotation that gains behaviour cannot go missing from the editor.")
        kw.appendLine("     */")
        kw.appendLine(strSet("ANNOTATIONS", strings("annotations")))
        kw.appendLine("}")
        pkgDir.resolve("JuxKeywords.kt").writeText(kw.toString())
    }
}

kotlin.sourceSets.named("main") {
    kotlin.srcDir(generateJuxTokens)
}

// ---------------------------------------------------------------------------
// The `jux.std` sources, bundled.
//
// The compiler embeds its standard library (`Option`, `Result`, the iterator
// combinators, `Mutex`, the exceptions, ...) as Jux source inside
// `crates/juxc-driver/src/stdlib_embedded.rs` and prepends it to every unit.
// This task writes those same sources out as `jux-std/jux/std/<path>.jux`
// resources, so the plugin indexes the exact library the compiler checks
// against: completion, navigation and Quick Documentation for every std
// member, with no second copy to keep in step by hand.
//
// `src/main/juxStdIde/` adds the few types the compiler binds structurally
// and so never writes as source (the range types of MISSING-DEFS M.6.1),
// declared exactly as that clause declares them.
// ---------------------------------------------------------------------------
val stdlibEmbedded = layout.projectDirectory.file("../../crates/juxc-driver/src/stdlib_embedded.rs")
val juxStdIdeDir = layout.projectDirectory.dir("src/main/juxStdIde")
val generatedJuxStdDir = layout.buildDirectory.dir("generated/resources/juxStd")

val generateJuxStd by tasks.registering {
    description = "Writes the compiler's embedded jux.std sources out as plugin resources."
    val input = stdlibEmbedded
    val ide = juxStdIdeDir
    val outDir = generatedJuxStdDir
    inputs.file(input)
    inputs.dir(ide)
    outputs.dir(outDir)

    doLast {
        val root = outDir.get().asFile.resolve("jux-std")
        root.deleteRecursively()
        val text = input.asFile.readText()
        // Each entry is `("dir/File.jux", r###"<source>"###)`.
        val entry = Regex("""\("([A-Za-z0-9_/]+\.jux)",\s*r###"(.*?)"###\)""", RegexOption.DOT_MATCHES_ALL)
        var count = 0
        for (m in entry.findAll(text)) {
            val file = root.resolve("jux/std/" + m.groupValues[1])
            file.parentFile.mkdirs()
            file.writeText(m.groupValues[2])
            count++
        }
        check(count > 0) { "no jux.std sources found in ${input.asFile}" }
        ide.asFile.walkTopDown().filter { it.isFile }.forEach { src ->
            val dest = root.resolve(src.relativeTo(ide.asFile).path)
            dest.parentFile.mkdirs()
            src.copyTo(dest, overwrite = true)
        }
    }
}

sourceSets.named("main") {
    resources.srcDir(generateJuxStd)
}
