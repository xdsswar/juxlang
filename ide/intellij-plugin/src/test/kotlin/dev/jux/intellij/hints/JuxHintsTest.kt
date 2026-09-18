package dev.jux.intellij.hints

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Inlay hints: parameter names in front of arguments, and the inferred type
 * of a `var` local.
 */
class JuxHintsTest : BasePlatformTestCase() {

    private val provider = JuxInlayHintsProvider()

    /** `name@argText` for every parameter hint in the file, in order. */
    private fun parameterHints(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        val text = myFixture.file.text
        val calls = PsiTreeUtil.collectElements(myFixture.file) {
            it.elementType === E.CALL_EXPRESSION || it.elementType === E.NEW_EXPRESSION
        }
        return calls.flatMap { provider.getParameterHints(it) }.map { hint ->
            val rest = text.substring(hint.offset)
            hint.text + "@" + rest.takeWhile { it != ',' && it != ')' }
        }
    }

    fun testMethodOfAnotherFileGetsHints() {
        myFixture.addFileToProject("geo.jux", "public class Canvas { public void draw(int width, int height) { } }")
        val hints = parameterHints("void main() { Canvas c = new Canvas(); c.draw(10, 20); }")
        assertEquals(listOf("width@10", "height@20"), hints)
    }

    fun testChainedCallGetsHints() {
        val hints = parameterHints(
            """
            public class Engine { public void start(int rpm) { } }
            public class Car { public Engine engine() { return new Engine(); } }
            void main() { Car c = new Car(); c.engine().start(900); }
            """.trimIndent(),
        )
        assertEquals(listOf("rpm@900"), hints)
    }

    fun testArgumentNamedLikeTheParameterGetsNoHint() {
        val hints = parameterHints(
            """
            public class Canvas { public void draw(int width, int height) { } }
            void main() { int width = 3; Canvas c = new Canvas(); c.draw(width, 20); }
            """.trimIndent(),
        )
        assertEquals(listOf("height@20"), hints)
    }

    fun testSameArityOverloadsOnlyHintWhereTheyAgree() {
        val hints = parameterHints(
            """
            public class Log {
                public void put(int level, int code) { }
                public void put(String level, String text) { }
            }
            void main() { Log l = new Log(); l.put(1, 2); }
            """.trimIndent(),
        )
        assertEquals(listOf("level@1"), hints)
    }

    fun testRecordConstructionGetsComponentNames() {
        val hints = parameterHints(
            """
            public record Point(int x, int y) { }
            void main() { Point p = new Point(3, 4); }
            """.trimIndent(),
        )
        assertEquals(listOf("x@3", "y@4"), hints)
    }

    // ---- var types ----

    private fun varHint(code: String, name: String): String? {
        myFixture.configureByText("b.jux", code)
        val local: PsiElement = PsiTreeUtil.collectElements(myFixture.file) {
            it.elementType === E.LOCAL_VARIABLE && (it as? dev.jux.intellij.psi.JuxNamedElement)?.name == name
        }.single()
        return JuxVarTypeHintsProvider.hintFor(local)
    }

    fun testVarShowsTheInferredType() {
        val code = """
            public class Shape { }
            public class Maker { public Shape make() { return new Shape(); } }
            void main() { Maker m = new Maker(); var s = m.make(); }
        """.trimIndent()
        assertEquals("Shape", varHint(code, "s"))
    }

    fun testVarWithAnObviousInitializerGetsNoHint() {
        assertNull(varHint("public class Shape { }\nvoid main() { var s = new Shape(); }", "s"))
        assertNull(varHint("void main() { var n = 42; }", "n"))
    }

    fun testWrittenTypeGetsNoHint() {
        assertNull(varHint("public class Shape { }\nvoid main() { Shape s = new Shape(); }", "s"))
    }
}
