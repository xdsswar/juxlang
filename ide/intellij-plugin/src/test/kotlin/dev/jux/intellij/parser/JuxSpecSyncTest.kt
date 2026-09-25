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

    // The for-each binder's `binding-modifier` (§A.2.8, ERRATA E95). All four
    // header shapes must read as valid, because the compiler now accepts all
    // four: a file that compiles must not be underlined as broken. The two
    // `final` rows are the ones that were a compiler parse error before, and the
    // two plain rows are here so a regression in either direction is visible.
    private fun forEach(header: String) =
        assertParses("package d; public class A { public void f() { $header { print(1); } } }")

    fun testForEachTypedBinder() { forEach("for (String? note : notes)") }
    fun testForEachVarBinder() { forEach("for (var note : notes)") }
    fun testForEachFinalVarBinder() { forEach("for (final var note : notes)") }
    fun testForEachFinalTypedBinder() { forEach("for (final String? note : notes)") }

    // `const` is a synonym of `final` wherever it appears (§A.2.2).
    fun testForEachConstVarBinder() { forEach("for (const var note : notes)") }
    fun testForEachConstTypedBinder() { forEach("for (const String note : notes)") }

    // `for await` has the same header shape (§18.6.3), so it takes the modifier
    // in the same position.
    fun testForAwaitFinalBinder() { forEach("for await (final int n : stream)") }
    fun testForAwaitFinalTypedBinder() { forEach("for await (final String? s : stream)") }

    // The disambiguation the modifier could have broken: a C-style header whose
    // init is `final` is still a C-style `for`, not a for-each.
    fun testFinalCStyleForStillParses() { forEach("for (final int i = 0; i < 3; i++)") }

    /**
     * Parsing the modifier is not enough: `final` has to land ON the binder's
     * declaration, because that is where [dev.jux.intellij.inspections.JuxAccess.modifiers]
     * reads it from. Left outside the [dev.jux.intellij.psi.JuxElementTypes.LOCAL_VARIABLE]
     * mark it would be a stray keyword, and every feature that asks whether a
     * binding is final (the reassignment check, the "may be final" hint, the
     * documentation popup) would read the for-each binder as mutable.
     */
    fun testTheForEachModifierSitsOnTheBinder() {
        myFixture.configureByText(
            "binder.jux",
            "package d; public class A { public void f() { for (final String? note : notes) { print(note); } } }",
        )
        val binder = PsiTreeUtil.collectElementsOfType(
            myFixture.file,
            dev.jux.intellij.psi.JuxLocalVariable::class.java,
        ).firstOrNull { it.name == "note" }
        assertNotNull("the for-each binder is a local variable declaration", binder)
        assertTrue(
            "`final` is part of the binder's declaration",
            "final" in dev.jux.intellij.inspections.JuxAccess.modifiers(binder!!),
        )
    }
}
