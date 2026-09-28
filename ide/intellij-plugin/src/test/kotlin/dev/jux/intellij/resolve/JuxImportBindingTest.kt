package dev.jux.intellij.resolve

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.inspections.JuxAbstractNotImplementedInspection
import dev.jux.intellij.inspections.JuxAmbiguousImportInspection
import dev.jux.intellij.inspections.JuxUnhandledExceptionInspection
import dev.jux.intellij.inspections.JuxUnresolvedReferenceInspection
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * What an import binds, as gaps 30-38 left the language (ERRATA E132-E134):
 * a grouped import decides between same-named types like a single one, a
 * crate family's nested packages (`rust.eframe.egui.Frame`) are packages of
 * their own, and the library exceptions a foreign error surfaces as
 * (`NumberFormatException`, `LibraryException`, `EncodingException`) are
 * ordinary `jux.std` classes.
 */
class JuxImportBindingTest : BasePlatformTestCase() {

    // ---- grouped imports ------------------------------------------------------

    /**
     * `examples/apps/orders`: `Main.jux` imports `orders.events.{Bus, Listener}`
     * and implements `Listener<Order>`; another program in the same source root
     * (`reentrant_callbacks.jux`) declares its own root-package `Listener` with
     * an `onClick`. The grouped import says which one is meant.
     */
    fun testGroupedImportDecidesBetweenSameNamedTypes() {
        myFixture.addFileToProject(
            "reentrant.jux",
            """
            interface Listener {
                void onClick();
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "orders/events/Listener.jux",
            """
            package orders.events;

            public interface Listener<T> {
                void on(T event);
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject("orders/events/Bus.jux", "package orders.events;\n\npublic class Bus<T> { }\n")
        myFixture.enableInspections(JuxAbstractNotImplementedInspection())
        myFixture.configureByText(
            "Main.jux",
            """
            import orders.events.{Bus, Listener};

            class Totals implements Listener<int> {
                public int count = 0;
                public void on(int event) { count++; }
            }
            """.trimIndent(),
        )
        val errors = myFixture.doHighlighting().filter { it.severity === HighlightSeverity.ERROR }.mapNotNull { it.description }
        assertEquals(emptyList<String>(), errors)
        val listener = JuxTypeIndex.findType(myFixture.file.firstChild, "Listener")
        assertEquals("orders.events", listener?.let { JuxAutoImport.packageOf(it) })
    }

    // ---- nested crate-family packages ---------------------------------------

    /** eframe's family stub and its nested `egui` package, in E132's shape. */
    private fun addEframeFamily() {
        myFixture.addFileToProject(
            "stubs/rust/eframe.jux.d",
            """
            package rust.eframe;

            public class Frame {
                public void close();
            }
            public class Ui {
                public void label(String text);
            }
            public class Panel {
                public Panel frame(rust.eframe.egui.Frame frame);
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "stubs/rust/eframe/egui.jux.d",
            """
            package rust.eframe.egui;

            import rust.eframe.*;

            public class Frame {
                public Frame();
                public Frame fill(int color);
            }
            public type Ui = rust.eframe.Ui;
            """.trimIndent(),
        )
    }

    fun testANestedFamilyPackageDeclaresItsOwnType() {
        addEframeFamily()
        myFixture.configureByText(
            "app.jux",
            """
            import rust.eframe.egui.Frame;
            import rust.eframe.egui.Ui;

            void draw(Ui ui) {
                var f = new Frame().fill(3);
                ui.label("hi");
                rust.eframe.Frame host = null;
            }
            """.trimIndent(),
        )
        val ctx = myFixture.file.firstChild
        val frame = JuxTypeIndex.findType(ctx, "Frame")
        assertEquals("rust.eframe.egui", frame?.let { JuxAutoImport.packageOf(it) })
        // The nested package's `Ui` is an alias of the host's: one type.
        val ui = JuxTypeIndex.findTypeThroughAliases(ctx, "Ui")
        assertEquals("rust.eframe", ui?.let { JuxAutoImport.packageOf(it) })
        val label = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.FIELD_ACCESS_EXPRESSION && it.text == "ui.label" }.single()
        assertNotNull("ui.label resolves through the alias", JuxTypeEngine.resolveMemberAccess(label, 1))
        val fill = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.FIELD_ACCESS_EXPRESSION && it.text.endsWith(".fill") }.single()
        assertNotNull("fill is egui's Frame's", JuxTypeEngine.resolveMemberAccess(fill, 1))
        // The fully-qualified host `Frame` is the host's, beside the imported egui one.
        val hostRef = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.TYPE_REFERENCE && it.text == "rust.eframe.Frame" }.single()
        val host = (JuxTypeEngine.typeOfTypeReference(hostRef) as? JuxType.ClassType)?.decl
        assertEquals("rust.eframe", host?.let { JuxAutoImport.packageOf(it) })
    }

    fun testTwoFamilyWildcardsAreAmbiguousOnlyForTwoTypes() {
        addEframeFamily()
        myFixture.enableInspections(JuxAmbiguousImportInspection())
        myFixture.configureByText(
            "app.jux",
            """
            import rust.eframe.*;
            import rust.eframe.egui.*;

            void draw(Ui ui) {
                Frame f = null;
            }
            """.trimIndent(),
        )
        val e0303 = myFixture.doHighlighting().mapNotNull { it.description }.filter { it.contains("E0303") }
        // `Frame` is two types; `Ui` is one type under two spellings.
        assertEquals(e0303.toString(), 1, e0303.size)
        assertTrue(e0303.single(), e0303.single().contains("'rust.eframe.Frame' and 'rust.eframe.egui.Frame'"))
    }

    fun testTwoSpellingsOfOneFamilyTypeAreOneImport() {
        addEframeFamily()
        myFixture.enableInspections(JuxAmbiguousImportInspection())
        myFixture.configureByText(
            "app.jux",
            """
            import rust.eframe.Ui;
            import rust.eframe.egui.Ui;
            import rust.eframe.Frame;
            import rust.eframe.egui.Frame;
            """.trimIndent(),
        )
        val e0303 = myFixture.doHighlighting().mapNotNull { it.description }.filter { it.contains("E0303") }
        // `Ui` is one type under two spellings; the two `Frame`s are two types.
        assertEquals(e0303.toString(), 1, e0303.size)
        assertTrue(e0303.single(), e0303.single().contains("'Frame'"))
    }

    // ---- ERRATA E134: the library exceptions --------------------------------

    fun testLibraryExceptionsAreStdClasses() {
        myFixture.enableInspections(JuxUnresolvedReferenceInspection(), JuxUnhandledExceptionInspection())
        myFixture.configureByText(
            "a.jux",
            """
            int parse(String s) {
                if (s == "") throw new NumberFormatException("empty");
                return 1;
            }
            void main() {
                try {
                    print(parse("x"));
                } catch (NumberFormatException e) {
                    print(e.getMessage());
                } catch (EncodingException e) {
                    print(e.getMessage());
                } catch (LibraryException e) {
                    print(e.getLibrary());
                }
            }
            """.trimIndent(),
        )
        val problems = myFixture.doHighlighting()
            .filter { it.severity.myVal >= HighlightSeverity.WARNING.myVal }
            .mapNotNull { it.description }
        assertEquals(emptyList<String>(), problems)
        val ctx = myFixture.file.firstChild
        for (name in listOf("NumberFormatException", "LibraryException", "EncodingException")) {
            val decl = JuxTypeIndex.findType(ctx, name)
            assertNotNull(name, decl)
            assertEquals(name, "jux.std.exceptions", JuxAutoImport.packageOf(decl!!))
        }
        assertTrue(JuxHierarchy.inheritsFrom(JuxTypeIndex.findType(ctx, "NumberFormatException")!!, "IllegalArgumentException"))
        assertTrue(JuxHierarchy.inheritsFrom(JuxTypeIndex.findType(ctx, "LibraryException")!!, "RuntimeException"))
        val getLibrary = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.FIELD_ACCESS_EXPRESSION && it.text == "e.getLibrary" }.single()
        assertNotNull(JuxTypeEngine.resolveMemberAccess(getLibrary, 0))
    }
}
