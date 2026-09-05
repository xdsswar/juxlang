package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * A bare type name that several files in the project declare differently must
 * resolve to the one in the file being edited.
 *
 * Jux compiles each file against its own imports, so the same name is free to
 * mean different things in different files — `examples/` alone declares
 * `Tagged` as an interface in one file and as a class in two others. Name
 * resolution used to take the first project-wide match in index order, so an
 * `interface Tagged` sitting three lines above its own implementer could
 * resolve to an unrelated `class Tagged` and every inspection built on top
 * reported a confident error about correct code: `implements Tagged` went red
 * with "cannot implement 'Tagged' because it is a class".
 */
class JuxSameNameAcrossFilesTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxInheritedTypeParamInspection(),
            JuxUnresolvedReferenceInspection(),
        )
    }

    /** A same-named CLASS in another file, so the project index has both. */
    private fun addCollidingClass() {
        myFixture.addFileToProject(
            "other.jux",
            """
            public class Node { public int id() { return 1; } }
            public class Tagged extends Node { }
            """.trimIndent(),
        )
    }

    fun testInterfaceInOwnFileWinsOverSameNamedClassElsewhere() {
        addCollidingClass()
        myFixture.configureByText(
            "a.jux",
            """
            public interface Tagged {
                int id();
                default String tag() { return "t" + this.id(); }
            }
            public abstract class Base implements Tagged {
                protected int n;
                public Base(int n) { this.n = n; }
                @Override public int id() { return this.n; }
                public String describe() { return this.tag(); }
            }
            public class Impl extends Base {
                public Impl(int n) { super(n); }
            }
            """.trimIndent(),
        )
        val errors = myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
        assertTrue("unexpected errors: $errors", errors.isEmpty())
    }

    fun testSameNamedClassElsewhereStillReachableWhenThisFileDeclaresNothing() {
        addCollidingClass()
        // No local `Tagged`, so the project-wide fallback must still find the
        // class in `other.jux` — and implementing a CLASS is a real E0424.
        myFixture.configureByText(
            "b.jux",
            """
            public class Uses implements Tagged { }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue("expected E0424 in $d", d.any { it.contains("E0424") })
    }
}
