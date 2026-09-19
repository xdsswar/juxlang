package dev.jux.intellij.format

import com.intellij.application.options.CodeStyle
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.codeStyle.CodeStyleSettingsCustomizable
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.codeStyle.LanguageCodeStyleSettingsProvider
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxLanguage

/**
 * The Wrapping and Braces and Blank Lines options the Jux page shows, each
 * checked against the formatter: an option on the page is an option Reformat
 * Code honors.
 */
class JuxCodeStyleOptionsTest : BasePlatformTestCase() {

    private fun reformat(before: String): String {
        myFixture.configureByText("a.jux", before)
        WriteCommandAction.runWriteCommandAction(project) {
            CodeStyleManager.getInstance(project).reformatText(myFixture.file, 0, myFixture.file.textLength)
        }
        return myFixture.file.text
    }

    /** Reformat [before] under the settings [configure] sets, then check it is stable. */
    private fun reformatWith(before: String, configure: (CommonCodeStyleSettings, JuxCodeStyleSettings) -> Unit): String {
        val settings = CodeStyle.getSettings(project).clone()
        configure(settings.getCommonSettings(JuxLanguage), settings.getCustomSettings(JuxCodeStyleSettings::class.java))
        var once = ""
        var twice = ""
        CodeStyle.doWithTemporarySettings(project, settings, Runnable {
            once = reformat(before)
            twice = reformat(once)
        })
        assertEquals("reformat is stable under these settings", once, twice)
        return once
    }

    private fun m(s: String) = s.trimMargin()

    // ---- Wrapping and Braces: 'else' / 'catch' / 'finally' / 'while' -------------

    fun testElseOnNewLine() {
        val src = m(
            """
            |void f(int a) {
            |    if (a > 0) {
            |        print(a);
            |    } else {
            |        print(0);
            |    }
            |}
            """,
        )
        assertEquals(
            m(
                """
                |void f(int a) {
                |    if (a > 0) {
                |        print(a);
                |    }
                |    else {
                |        print(0);
                |    }
                |}
                """,
            ),
            reformatWith(src) { c, _ -> c.ELSE_ON_NEW_LINE = true },
        )
    }

    fun testElseJoinsTheBraceWhenOff() {
        val src = m(
            """
            |void f(int a) {
            |    if (a > 0) {
            |        print(a);
            |    }
            |    else {
            |        print(0);
            |    }
            |}
            """,
        )
        assertTrue(reformatWith(src) { c, _ -> c.ELSE_ON_NEW_LINE = false }.contains("    } else {\n"))
    }

    fun testCatchFinallyAndWhileOnNewLine() {
        val src = m(
            """
            |void f() {
            |    try {
            |        g();
            |    } catch (Error e) {
            |        h();
            |    } finally {
            |        k();
            |    }
            |    do {
            |        g();
            |    } while (x);
            |}
            """,
        )
        val out = reformatWith(src) { c, _ ->
            c.CATCH_ON_NEW_LINE = true
            c.FINALLY_ON_NEW_LINE = true
            c.WHILE_ON_NEW_LINE = true
        }
        assertTrue(out, out.contains("    }\n    catch (Error e) {"))
        assertTrue(out, out.contains("    }\n    finally {"))
        assertTrue(out, out.contains("    }\n    while (x);"))
    }

    // ---- Wrapping and Braces: braces placement ----------------------------------

    fun testMethodAndClassBracesOnNextLine() {
        val src = m(
            """
            |class A {
            |    void m() {
            |        if (x) {
            |            g();
            |        }
            |    }
            |}
            """,
        )
        assertEquals(
            m(
                """
                |class A
                |{
                |    void m()
                |    {
                |        if (x) {
                |            g();
                |        }
                |    }
                |}
                """,
            ),
            reformatWith(src) { _, j ->
                j.CLASS_BRACE_STYLE = JuxCodeStyleSettings.NEXT_LINE
                j.METHOD_BRACE_STYLE = JuxCodeStyleSettings.NEXT_LINE
            },
        )
    }

    fun testOtherBracesOnNextLine() {
        val out = reformatWith(m("|void m() {\n|    if (x) {\n|        g();\n|    }\n|}")) { _, j ->
            j.BRACE_STYLE = JuxCodeStyleSettings.NEXT_LINE
        }
        assertEquals(m("|void m() {\n|    if (x)\n|    {\n|        g();\n|    }\n|}"), out)
    }

    fun testEndOfLineJoinsANextLineBrace() {
        val out = reformatWith(m("|void m()\n|{\n|    g();\n|}")) { _, _ -> }
        assertEquals(m("|void m() {\n|    g();\n|}"), out)
    }

    fun testNextLineIfWrapped() {
        val src = m(
            """
            |void wrapped(int a,
            |        int b) {
            |    g();
            |}
            |void flat(int a) {
            |    g();
            |}
            """,
        )
        val out = reformatWith(src) { _, j -> j.METHOD_BRACE_STYLE = JuxCodeStyleSettings.NEXT_LINE_IF_WRAPPED }
        assertTrue(out, out.contains("        int b)\n{"))
        assertTrue(out, out.contains("void flat(int a) {"))
    }

    fun testLambdaBraceStyle() {
        val out = reformatWith(m("|void m() {\n|    var f = (int x) -> {\n|        return x;\n|    };\n|}")) { _, j ->
            j.LAMBDA_BRACE_STYLE = JuxCodeStyleSettings.NEXT_LINE
        }
        assertTrue(out, out.contains("(int x) ->\n    {"))
    }

    // ---- Wrapping and Braces: keep when reformatting ----------------------------

    fun testSimpleMethodsKeptOnOneLineByDefault() {
        val src = "class A {\n    public int get() { return 1; }\n}"
        assertEquals(src, reformat(src))
    }

    fun testSimpleMethodSplitWhenNotKept() {
        val out = reformatWith("class A {\n    public int get() { return 1; }\n}") { c, _ -> c.KEEP_SIMPLE_METHODS_IN_ONE_LINE = false }
        assertEquals("class A {\n    public int get() {\n        return 1;\n    }\n}", out)
    }

    fun testSimpleClassSplitWhenNotKept() {
        val out = reformatWith("interface Shape { double area(); String name(); }") { c, _ -> c.KEEP_SIMPLE_CLASSES_IN_ONE_LINE = false }
        assertEquals("interface Shape {\n    double area();\n    String name();\n}", out)
    }

    fun testSimpleBlockAndLambdaSplitWhenNotKept() {
        val src = "void m() {\n    if (x) { g(); }\n    var f = () -> { g(); };\n}"
        val blocks = reformatWith(src) { c, _ -> c.KEEP_SIMPLE_BLOCKS_IN_ONE_LINE = false }
        assertTrue(blocks, blocks.contains("if (x) {\n        g();\n    }"))
        assertTrue("the lambda keeps its own option: $blocks", blocks.contains("() -> { g(); }"))
        val lambdas = reformatWith(src) { c, _ -> c.KEEP_SIMPLE_LAMBDAS_IN_ONE_LINE = false }
        assertTrue(lambdas, lambdas.contains("() -> {\n        g();\n    };"))
    }

    fun testCommentAtFirstColumn() {
        val src = "void m() {\n// off\n    g();\n}"
        assertEquals(src, reformatWith(src) { c, _ -> c.KEEP_FIRST_COLUMN_COMMENT = true })
        assertEquals("void m() {\n    // off\n    g();\n}", reformatWith(src) { c, _ -> c.KEEP_FIRST_COLUMN_COMMENT = false })
    }

    // ---- Wrapping and Braces: force braces --------------------------------------

    fun testForceBracesAlways() {
        val src = m(
            """
            |void m(int a) {
            |    if (a > 0) g();
            |    else if (a < 0) h();
            |    else k();
            |    for (int i = 0; i < a; i++) g();
            |    while (a > 0) a = a - 1;
            |    do g(); while (a > 0);
            |}
            """,
        )
        val out = reformatWith(src) { c, _ ->
            c.IF_BRACE_FORCE = CommonCodeStyleSettings.FORCE_BRACES_ALWAYS
            c.FOR_BRACE_FORCE = CommonCodeStyleSettings.FORCE_BRACES_ALWAYS
            c.WHILE_BRACE_FORCE = CommonCodeStyleSettings.FORCE_BRACES_ALWAYS
            c.DOWHILE_BRACE_FORCE = CommonCodeStyleSettings.FORCE_BRACES_ALWAYS
        }
        assertEquals(
            m(
                """
                |void m(int a) {
                |    if (a > 0) {
                |        g();
                |    } else if (a < 0) {
                |        h();
                |    } else {
                |        k();
                |    }
                |    for (int i = 0; i < a; i++) {
                |        g();
                |    }
                |    while (a > 0) {
                |        a = a - 1;
                |    }
                |    do {
                |        g();
                |    } while (a > 0);
                |}
                """,
            ),
            out,
        )
    }

    fun testForceBracesIfMultiline() {
        val src = "void m(int a) {\n    if (a > 0) g();\n    if (a > 1)\n        h();\n}"
        val out = reformatWith(src) { c, _ -> c.IF_BRACE_FORCE = CommonCodeStyleSettings.FORCE_BRACES_IF_MULTILINE }
        assertEquals("void m(int a) {\n    if (a > 0) g();\n    if (a > 1) {\n        h();\n    }\n}", out)
    }

    fun testNoBracesForcedByDefault() {
        val src = "void m(int a) {\n    if (a > 0) g();\n}"
        assertEquals(src, reformat(src))
    }

    // ---- Blank Lines ----------------------------------------------------------

    fun testBlankLinesAroundMethodAndField() {
        val src = "class A {\n    int x;\n    int y;\n    void a() {}\n    void b() {}\n}"
        val out = reformatWith(src) { c, _ ->
            c.BLANK_LINES_AROUND_METHOD = 1
            c.BLANK_LINES_AROUND_FIELD = 0
        }
        assertEquals("class A {\n    int x;\n    int y;\n\n    void a() {}\n\n    void b() {}\n}", out)
    }

    fun testBlankLinesInInterfaceUseTheirOwnOptions() {
        val src = "interface I {\n    void a();\n    void b();\n}"
        val out = reformatWith(src) { c, _ ->
            c.BLANK_LINES_AROUND_METHOD = 1
            c.BLANK_LINES_AROUND_METHOD_IN_INTERFACE = 0
        }
        assertEquals(src, out)
    }

    fun testOneLinerIgnoresMinimumBlankLines() {
        val src = "interface Shape { double area(); String name(); }"
        assertEquals(src, reformatWith(src) { c, _ -> c.BLANK_LINES_AROUND_METHOD_IN_INTERFACE = 1 })
    }

    fun testHeaderBlankLines() {
        val src = "package a;\nimport b.C;\nimport b.D;\nclass A {}\nclass B {}"
        val out = reformatWith(src) { c, _ ->
            c.BLANK_LINES_AFTER_PACKAGE = 1
            c.BLANK_LINES_BEFORE_IMPORTS = 1
            c.BLANK_LINES_AFTER_IMPORTS = 1
            c.BLANK_LINES_AROUND_CLASS = 2
        }
        assertEquals("package a;\n\nimport b.C;\nimport b.D;\n\nclass A {}\n\n\nclass B {}", out)
    }

    fun testClassHeaderEndAndMethodBody() {
        val src = "class A {\n    void m() {\n        g();\n    }\n}"
        val out = reformatWith(src) { c, _ ->
            c.BLANK_LINES_AFTER_CLASS_HEADER = 1
            c.BLANK_LINES_BEFORE_CLASS_END = 1
            c.BLANK_LINES_BEFORE_METHOD_BODY = 1
        }
        assertEquals("class A {\n\n    void m() {\n\n        g();\n    }\n\n}", out)
    }

    fun testKeepBlankLinesBeforeRbrace() {
        val src = "void m() {\n    g();\n\n\n}"
        assertEquals("void m() {\n    g();\n}", reformatWith(src) { c, _ -> c.KEEP_BLANK_LINES_BEFORE_RBRACE = 0 })
    }

    // ---- the page -------------------------------------------------------------

    fun testThePageShowsJavasTabs() {
        val provider = LanguageCodeStyleSettingsProvider.forLanguage(JuxLanguage)!!
        fun shown(type: LanguageCodeStyleSettingsProvider.SettingsType): Set<String> {
            val names = HashSet<String>()
            provider.customizeSettings(
                object : CodeStyleSettingsCustomizable {
                    override fun showAllStandardOptions() = fail("the page shows chosen options only")

                    override fun showStandardOptions(vararg optionNames: String) {
                        names += optionNames
                    }

                    override fun showCustomOption(
                        settingsClass: Class<out com.intellij.psi.codeStyle.CustomCodeStyleSettings>,
                        fieldName: String,
                        title: String,
                        groupName: String?,
                        vararg options: Any,
                    ) {
                        names += fieldName
                    }
                },
                type,
            )
            return names
        }
        val wrapping = shown(LanguageCodeStyleSettingsProvider.SettingsType.WRAPPING_AND_BRACES_SETTINGS)
        assertTrue(wrapping.toString(), wrapping.containsAll(listOf("ELSE_ON_NEW_LINE", "KEEP_SIMPLE_METHODS_IN_ONE_LINE", "CLASS_BRACE_STYLE", "BRACE_STYLE")))
        val blank = shown(LanguageCodeStyleSettingsProvider.SettingsType.BLANK_LINES_SETTINGS)
        assertTrue(blank.toString(), blank.containsAll(listOf("BLANK_LINES_AROUND_METHOD", "BLANK_LINES_AFTER_IMPORTS", "KEEP_BLANK_LINES_BEFORE_RBRACE")))
    }
}
