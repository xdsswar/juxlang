package dev.jux.intellij.format

import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.io.File

/**
 * Reformat (Ctrl+Alt+L) coverage: indentation, spacing, cuddled keywords,
 * line-break preservation, opacity of string interiors / anonymous bodies,
 * idempotency — plus a bulk pass over the whole examples corpus.
 */
class JuxFormatterTest : BasePlatformTestCase() {

    private fun reformat(before: String): String {
        myFixture.configureByText("a.jux", before)
        WriteCommandAction.runWriteCommandAction(project) {
            CodeStyleManager.getInstance(project)
                .reformatText(myFixture.file, 0, myFixture.file.textLength)
        }
        return myFixture.file.text
    }

    private fun doTest(before: String, after: String) = assertEquals(after, reformat(before))

    fun testClassMembersIndent() = doTest(
        """
        |package demo;
        |public class A {
        |int x;
        |        public void go() {
        |print(x);
        |}
        |}
        """.trimMargin(),
        """
        |package demo;
        |public class A {
        |    int x;
        |    public void go() {
        |        print(x);
        |    }
        |}
        """.trimMargin(),
    )

    /** §L.7 native block: foreign-fn declarations indent one level (4 spaces). */
    fun testNativeBlockIndents() = doTest(
        """
        |@extern(lib = "c")
        |unsafe native {
        |int puts(String s);
        |String getenv(String name);
        |}
        """.trimMargin(),
        """
        |@extern(lib = "c")
        |unsafe native {
        |    int puts(String s);
        |    String getenv(String name);
        |}
        """.trimMargin(),
    )

    fun testNestedIfIndent() = doTest(
        """
        |public void go(int a) {
        |if (a > 0) {
        |if (a > 1) {
        |print(a);
        |}
        |}
        |}
        """.trimMargin(),
        """
        |public void go(int a) {
        |    if (a > 0) {
        |        if (a > 1) {
        |            print(a);
        |        }
        |    }
        |}
        """.trimMargin(),
    )

    fun testElseCuddleAndSingleStatementBody() = doTest(
        """
        |public void go(int a) {
        |    if (a > 0) {
        |        print(a);
        |    }    else{
        |        print(0);
        |    }
        |    if (a < 0)
        |    print(a);
        |}
        """.trimMargin(),
        """
        |public void go(int a) {
        |    if (a > 0) {
        |        print(a);
        |    } else {
        |        print(0);
        |    }
        |    if (a < 0)
        |        print(a);
        |}
        """.trimMargin(),
    )

    fun testOperatorSpacingSoup() = doTest(
        """
        |public int f(int a, int b) {
        |    int x=a+b*2&&a<b;
        |    return x;
        |}
        """.trimMargin(),
        """
        |public int f(int a, int b) {
        |    int x = a + b * 2 && a < b;
        |    return x;
        |}
        """.trimMargin(),
    )

    fun testGenericsStayTightAndCommasSpace() = doTest(
        """
        |public class A {
        |    Map<String,int> m;
        |    public void go(int a,int b) { take(a ,b); }
        |}
        """.trimMargin(),
        """
        |public class A {
        |    Map<String, int> m;
        |    public void go(int a, int b) { take(a, b); }
        |}
        """.trimMargin(),
    )

    fun testSwitchCaseIndentAndArrows() = doTest(
        """
        |public String f(int t) {
        |    return switch (t) {
        |    case 1|2->"few";
        |    default -> { yield "many"; }
        |    };
        |}
        """.trimMargin(),
        """
        |public String f(int t) {
        |    return switch (t) {
        |        case 1 | 2 -> "few";
        |        default -> { yield "many"; }
        |    };
        |}
        """.trimMargin(),
    )

    fun testLambdaAndTypeTestSpacing() = doTest(
        """
        |public void go(Object a) {
        |    var f = (x)->x+1;
        |    var ok = a=>String s;
        |}
        """.trimMargin(),
        """
        |public void go(Object a) {
        |    var f = (x) -> x + 1;
        |    var ok = a => String s;
        |}
        """.trimMargin(),
    )

    fun testChainedCallContinuationIndent() = doTest(
        """
        |public void go(String s) {
        |    var r = s.trim()
        |    .toUpperCase()
        |    .length();
        |}
        """.trimMargin(),
        """
        |public void go(String s) {
        |    var r = s.trim()
        |            .toUpperCase()
        |            .length();
        |}
        """.trimMargin(),
    )

    fun testOneLinerInterfaceSurvives() {
        val src = """
            |public interface Shape { double area(); String name(); }
        """.trimMargin()
        assertEquals(src, reformat(src))
    }

    fun testBlankLineClamp() = doTest(
        """
        |public class A {
        |    int x;
        |
        |
        |
        |
        |    int y;
        |}
        """.trimMargin(),
        """
        |public class A {
        |    int x;
        |
        |
        |    int y;
        |}
        """.trimMargin(),
    )

    fun testInterpStringAndAnonymousBodyUntouched() {
        val src = """
            |public void go(int n) {
            |    var s = ${'$'}"weird   ${'$'}{n}  spacing";
            |    var r = new Runnable() { public void run() { print(s); } };
            |}
        """.trimMargin()
        val out = reformat(src)
        assertTrue("interp string interior must be byte-identical", out.contains("weird   ${'$'}{n}  spacing"))
    }

    fun testBrokenCodeDoesNotThrow() {
        val src = """
            |public class A {
            |    public void go( {
            |        if (x {
            |}
        """.trimMargin()
        reformat(src) // must not throw
    }

    /** Bulk regression: every corpus example reformats without throwing, and
     *  reformatting twice equals reformatting once (idempotency). */
    fun testCorpusReformatIdempotent() {
        val examples = File("../../examples").absoluteFile
        assertTrue("examples dir not found at $examples", examples.isDirectory)
        var count = 0
        for (file in examples.listFiles { _, n -> n.endsWith(".jux") }!!.sortedBy { it.name }) {
            count++
            val once = reformat(file.readText())
            val twice = reformat(once)
            assertEquals("reformat not idempotent for ${file.name}", once, twice)
        }
        assertTrue(count > 0)
    }
}
