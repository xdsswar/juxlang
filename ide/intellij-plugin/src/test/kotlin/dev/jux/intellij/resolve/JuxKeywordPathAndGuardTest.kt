package dev.jux.intellij.resolve

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxNullableAccessInspection
import dev.jux.intellij.inspections.JuxPackageMismatchInspection
import dev.jux.intellij.inspections.JuxRedundantNullCheckInspection
import dev.jux.intellij.inspections.JuxUnreachableCodeInspection

/**
 * Two compiler changes the editor has to keep step with.
 *
 * **ERRATA E78, a Jux keyword as a path segment.** Every segment of a
 * qualified name after the first is a member-position name, so
 * `import rust.x.record;` and `package demo.type;` are legal: a Rust crate
 * really does have modules called `record`, `type` and `drop`, and before
 * this they were unimportable. The parser already reads them (it remaps the
 * keyword leaf to an identifier); what is pinned here is everything built on
 * top of that: the import RESOLVES, and the package check does not call a
 * legal path a mismatch.
 *
 * **ERRATA E79, a guard clause that leaves with `break` or `continue`.** A
 * null test that exits the block narrows the name for the rest of it, exactly
 * as a `return` guard already did. The plugin models nullability without a
 * flow graph, so what matters here is that nothing it reports contradicts the
 * compiler: the uses after such a guard must stay clean.
 */
class JuxKeywordPathAndGuardTest : BasePlatformTestCase() {

    // ---- ERRATA E78: a keyword path segment ---------------------------------

    fun testATypeImportedThroughAKeywordSegmentResolves() {
        myFixture.addFileToProject(
            "rust/x/record/Ticket.jux",
            "package rust.x.record;\npublic class Ticket {\n    public int id = 0;\n}\n",
        )
        myFixture.configureByText(
            "a.jux",
            """
            import rust.x.record.Tic<caret>ket;

            void main() {}
            """.trimIndent(),
        )
        val target = myFixture.getReferenceAtCaretPosition()?.resolve()
        assertNotNull("a type behind a `record` path segment resolves", target)
        assertEquals("Ticket", (target as? dev.jux.intellij.psi.JuxNamedElement)?.name)
    }

    fun testATypeUsedThroughAKeywordSegmentImportResolves() {
        myFixture.addFileToProject(
            "rust/x/record/Ticket.jux",
            "package rust.x.record;\npublic class Ticket {\n    public int id = 0;\n}\n",
        )
        myFixture.configureByText(
            "a.jux",
            """
            import rust.x.record.Ticket;

            void main() {
                Tic<caret>ket t = new Ticket();
            }
            """.trimIndent(),
        )
        val target = myFixture.getReferenceAtCaretPosition()?.resolve()
        assertNotNull("the imported name resolves at its use", target)
        assertEquals("Ticket", (target as? dev.jux.intellij.psi.JuxNamedElement)?.name)
    }

    fun testAPackageWithAKeywordSegmentIsNotAMismatch() {
        myFixture.enableInspections(JuxPackageMismatchInspection())
        myFixture.addFileToProject(
            "demo/type/Holder.jux",
            "package demo.type;\npublic class Holder {}\n",
        )
        myFixture.configureByFile("demo/type/Holder.jux")
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue("`package demo.type;` matches its directory: $d", d.none { it.contains("package") })
    }

    fun testASiblingBehindAKeywordSegmentNeedsNoImport() {
        myFixture.addFileToProject("Holder.jux", "package demo.type;\npublic class Holder {}\n")
        myFixture.configureByText(
            "b.jux",
            """
            package demo.type;

            void main() {
                Hol<caret>der h = new Holder();
            }
            """.trimIndent(),
        )
        // The declared `package demo.type;` is what puts the two files in one
        // package, keyword segment and all, so the name resolves with no
        // `import` at all.
        val target = myFixture.getReferenceAtCaretPosition()?.resolve()
        assertNotNull("a sibling in package `demo.type` resolves", target)
        assertEquals("Holder", (target as? dev.jux.intellij.psi.JuxNamedElement)?.name)
    }

    // ---- ERRATA E79: a guard that leaves with break / continue --------------

    private fun descriptions(code: String): List<String> {
        myFixture.enableInspections(
            JuxNullableAccessInspection(),
            JuxRedundantNullCheckInspection(),
            JuxUnreachableCodeInspection(),
        )
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    fun testAContinueGuardNarrowsForTheRestOfTheLoopBody() {
        val d = descriptions(
            """
            void main(Vec<String?> items) {
                for (String? s : items) {
                    if (s == null) continue;
                    print(s.length());
                }
            }
            """,
        )
        assertTrue("nothing contradicts the continue guard: $d", d.isEmpty())
    }

    fun testABreakGuardNarrowsForTheRestOfTheLoopBody() {
        val d = descriptions(
            """
            void main(Vec<String?> items) {
                for (String? s : items) {
                    if (s == null) break;
                    print(s.length());
                }
            }
            """,
        )
        assertTrue("nothing contradicts the break guard: $d", d.isEmpty())
    }

    fun testAReturnGuardStillNarrows() {
        val d = descriptions(
            """
            void show(String? s) {
                if (s == null) return;
                print(s.length());
            }
            """,
        )
        assertTrue("the return guard stays clean: $d", d.isEmpty())
    }

    fun testAnUnguardedNullableUseIsStillReported() {
        val d = descriptions(
            """
            void show(String? s) {
                print(s.length());
            }
            """,
        )
        // The guard rule widening must not silence the case it exists for.
        assertTrue("an unchecked nullable use is still flagged: $d", d.any { it.contains("may be null") })
    }
}
