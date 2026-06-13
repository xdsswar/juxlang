package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Spec-sync regression guard: constructs the compiler accepts that the plugin's
 * parser must NOT red-flag (JUX-GRAMMAR-ADDENDUM, §M.14, §7.4.3). Asserts each
 * parses with zero [PsiErrorElement]s. A failure here means a parser change has
 * started rejecting valid syntax (e.g. tightening unary parsing would break the
 * `++`/`--` cases, which currently lex as repeated `+`/`-`).
 */
class JuxSpecSyncTest : BasePlatformTestCase() {

    private fun assertParses(code: String) {
        myFixture.configureByText("p.jux", code)
        val errs = PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java)
            .map { "${it.errorDescription}@${it.textOffset}" }
        assertEmpty("parse errors in: $code -> $errs", errs)
    }

    // Multi-dimensional array types (§A.2.7).
    fun testArray2d() { assertParses("package d; public class A { public int[][] m; }") }
    fun testArray3d() { assertParses("package d; public class A { public int[][][] m; }") }
    fun testArraySized() { assertParses("package d; public class A { public int[3][4] m; }") }
    fun testArrayMixed() { assertParses("package d; public class A { public int[3][] m; }") }

    // Expression-position ++ / -- (prefix and postfix).
    fun testPostInc() { assertParses("package d; public class A { public void f(int x) { print(x++); } }") }
    fun testPreInc() { assertParses("package d; public class A { public void f(int x) { var y = ++x; } }") }
    fun testIndexInc() { assertParses("package d; public class A { public void f() { arr[i++] = 1; } }") }
    fun testReturnDec() { assertParses("package d; public class A { public int f(int n) { return n--; } }") }

    // Parameter binding-mode combinations (§M.14).
    fun testFinalRef() { assertParses("package d; public class A { public void f(final ref int n) {} }") }
    fun testFinalWeak() { assertParses("package d; public class A { public void f(final weak Node n) {} }") }
    fun testFinalDefault() { assertParses("package d; public class A { public void f(final int x = 1) {} }") }
    fun testFinalVarargs() { assertParses("package d; public class A { public void f(final int... xs) {} }") }

    // Interface method without a body or explicit visibility (§7.4.3 public by default).
    fun testInterfaceMethod() { assertParses("package d; public interface I { String name(); }") }
}
