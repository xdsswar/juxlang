package dev.jux.intellij.resolve

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxUnresolvedReferenceInspection

/**
 * A generated `.jux.d` stub is Jux source, so the toolchain's standard library
 * and every bound crate become ordinary declarations once indexed: they
 * resolve, they complete, and go-to-definition opens them.
 *
 * That is the whole design — the plugin discovers the library surface from the
 * installed compiler ([JuxStubRoots] / [JuxLibraryRootsProvider]) instead of
 * carrying a description of it, which would be wrong for whichever crates the
 * user happens to depend on and stale the first time the standard library
 * changed.
 *
 * The fixture puts the stub inside the project (an `AdditionalLibraryRootsProvider`
 * needs a real directory outside it), which exercises everything downstream of
 * discovery: the `*.jux.d` file-type pattern, the parser, the PSI, the type
 * index, and the inspections that read it.
 */
class JuxStubIndexTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxUnresolvedReferenceInspection())
    }

    /** The shape the compiler's bindgen actually emits for a bound crate. */
    private fun addCrateStub() {
        myFixture.addFileToProject(
            ".jux-stubs/rust/demo.jux.d",
            """
            package rust.demo;

            public class Widget {
                public int width();
                public void resize(int w);
            }
            """.trimIndent(),
        )
    }

    fun testStubTypeResolvesLikeAnyOtherDeclaration() {
        addCrateStub()
        myFixture.configureByText(
            "a.jux",
            """
            import rust.demo.*;
            void main() {
                Widget w = new Widget();
                print(w.width());
            }
            """.trimIndent(),
        )
        val errors = myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
        assertTrue("stub type should resolve, got: $errors", errors.isEmpty())
    }

    fun testStubTypeIsOfferedBelowTheUsersOwnTypes() {
        // Names chosen to share a prefix of the same length, so the platform's
        // own prefix matching cannot decide the order and the P_TYPE_* tiers
        // do. (A name the user is literally typing SHOULD win on prefix length;
        // that is the platform's job and this test is not about it.)
        myFixture.addFileToProject(
            ".jux-stubs/rust/zed.jux.d",
            "package rust.zed;\n\npublic class Zenith { }\n",
        )
        myFixture.addFileToProject("mine.jux", "public class Zephyr { }")
        myFixture.configureByText("b.jux", "void main() { Ze<caret> }")
        val offered = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
        assertTrue("expected the stub type among $offered", offered.contains("Zenith"))
        assertTrue("expected the project type among $offered", offered.contains("Zephyr"))
        assertTrue(
            "the user's own type should outrank the library's: $offered",
            offered.indexOf("Zephyr") < offered.indexOf("Zenith"),
        )
    }

    fun testStubMembersComplete() {
        addCrateStub()
        myFixture.configureByText(
            "c.jux",
            """
            import rust.demo.*;
            void main() { Widget w = new Widget(); w.<caret> }
            """.trimIndent(),
        )
        val offered = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
        assertTrue("expected the stub's members among $offered", offered.containsAll(listOf("width", "resize")))
    }

    fun testRecordComponentsCompleteAsMembers() {
        myFixture.configureByText(
            "d.jux",
            """
            public record Pt(int x, int y) { }
            void main() { Pt p = new Pt(1, 2); p.<caret> }
            """.trimIndent(),
        )
        val offered = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
        assertTrue("record components should complete as members: $offered", offered.containsAll(listOf("x", "y")))
    }

    /** Discovery must never throw on a machine with no toolchain installed. */
    fun testDiscoveryIsSafeWithNoToolchain() {
        val roots = JuxStubRoots.externalRoots()
        roots.forEach { assertTrue("reported a non-directory: $it", it.isDirectory) }
        JuxStubRoots.projectStubDirs(project) // must not throw
    }
}
