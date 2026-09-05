package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The §P property surface, which the `examples/` corpus barely exercises — zero
 * examples declare a custom `set { }` body, so the corpus highlighting test
 * cannot see any of this.
 *
 * Both cases here are code the compiler accepts (verified against the release
 * toolchain) and the editor used to paint red.
 */
class JuxPropertySurfaceTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxAccessorVisibilityInspection(),
            JuxImplementsClauseInspection(),
            JuxMisplacedAccessorBlockInspection(),
            JuxUnresolvedReferenceInspection(),
        )
    }

    private fun errors(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
            .distinct()
    }

    /**
     * `value` is the implicit setter parameter (§P.1.4). It is bound only inside
     * an accessor body, and the compiler routes the write through it — the
     * editor flagged it as an unresolved symbol.
     */
    fun testImplicitSetterValueResolves() {
        val d = errors(
            """
            public class Widget {
                private String t = "";
                public String Title {
                    get { return this.t; }
                    set { if (value == "") { this.t = "(empty)"; } else { this.t = value; } }
                }
            }
            """.trimIndent(),
        )
        assertTrue("`value` should resolve inside an accessor body: $d", d.isEmpty())
    }

    /** Outside an accessor `value` is an ordinary name and must NOT be special-cased. */
    fun testValueOutsideAnAccessorIsStillUnresolved() {
        myFixture.configureByText(
            "b.jux",
            """
            public class Widget {
                public int go() { return value; }
            }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue(
            "`value` outside an accessor is an ordinary unresolved name: $d",
            d.any { it.contains("Cannot resolve symbol 'value'") },
        )
    }

    /**
     * A property satisfies an interface's accessor method — `interface Named {
     * String Name(); }` is implemented by `public String Name -> "n";` with no
     * method declared. The override engine counted only methods and record
     * components as provided, so this raised a false E0429.
     */
    fun testPropertySatisfiesAnInterfaceAccessor() {
        val d = errors(
            """
            public interface Named { String Name(); }
            public class Tagged implements Named {
                public String Name -> "n";
            }
            """.trimIndent(),
        )
        assertTrue("a property implements the interface's accessor: $d", d.isEmpty())
    }

    /** The full-accessor form satisfies it too. */
    fun testFullAccessorPropertySatisfiesAnInterfaceAccessor() {
        val d = errors(
            """
            public interface Named { String Name(); }
            public class Tagged implements Named {
                private String n = "x";
                public String Name { get { return this.n; } set { this.n = value; } }
            }
            """.trimIndent(),
        )
        assertTrue("a full-accessor property implements the accessor: $d", d.isEmpty())
    }

    /** A genuinely missing member must still be reported — the fix must not blanket-silence. */
    fun testMissingMemberIsStillReported() {
        val d = errors(
            """
            public interface Named { String Name(); int Size(); }
            public class Tagged implements Named {
                public String Name -> "n";
            }
            """.trimIndent(),
        )
        assertTrue("the unimplemented `Size` must still be flagged: $d", d.any { it.contains("E0429") })
    }
}
