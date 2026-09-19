package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.navigation.actions.GotoTypeDeclarationAction
import com.intellij.ide.util.InheritedMembersNodeProvider
import com.intellij.lang.LanguageExpressionTypes
import com.intellij.psi.util.elementType
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.testIntegration.LanguageTestCreators
import com.intellij.testIntegration.TestFinder
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.run.JuxTestCreator
import dev.jux.intellij.run.JuxTestFinder

/**
 * Java's navigation and info actions on Jux: Type Info (Ctrl+Shift+P), Type
 * Declaration (Ctrl+Shift+B), the File Structure toggles (Ctrl+F12), and
 * Go to Test / Create Test (Ctrl+Shift+T).
 */
class JuxNavigationActionsTest : BasePlatformTestCase() {

    // ---- Type Info --------------------------------------------------------

    /** The hints Ctrl+Shift+P offers at the caret, innermost expression first. */
    private fun typeInfo(text: String): List<String> {
        myFixture.configureByText("a.jux", text)
        val provider = LanguageExpressionTypes.INSTANCE.forLanguage(JuxLanguage)
        assertNotNull("Type Info is registered for Jux", provider)
        @Suppress("UNCHECKED_CAST")
        val p = provider as com.intellij.lang.ExpressionTypeProvider<com.intellij.psi.PsiElement>
        val leaf = myFixture.file.findElementAt(myFixture.caretOffset)!!
        // The hint is HTML; compare the text it shows.
        return p.getExpressionsAt(leaf).map { com.intellij.openapi.util.text.StringUtil.unescapeXmlEntities(p.getInformationHint(it)) }
    }

    fun testTypeInfoOfALocal() {
        val hints = typeInfo(
            """
            class Box<T> { public T item; public Box(T item) { this.item = item; } }
            void main() { Box<int> b = new Box<int>(1); print(<caret>b); }
            """.trimIndent(),
        )
        assertEquals("Box<int>", hints.first())
    }

    fun testTypeInfoOfNestedExpressions() {
        val hints = typeInfo(
            """
            class Truck { public int load() { return 3; } }
            class Fleet { public Truck lead = new Truck(); }
            void main() { var f = new Fleet(); print(f.le<caret>ad.load()); }
            """.trimIndent(),
        )
        assertEquals(listOf("Truck", "int"), hints)
    }

    fun testTypeInfoOfATypeTestIsBool() {
        val hints = typeInfo("class A {}\nclass B extends A {}\nvoid main() { A a = new B(); var ok = a =<caret>> B; }")
        assertEquals("bool", hints.first())
    }

    // ---- Type Declaration -------------------------------------------------

    private fun typeDeclaration(text: String): String? {
        myFixture.configureByText("a.jux", text)
        val types = GotoTypeDeclarationAction.findSymbolTypes(myFixture.editor, myFixture.caretOffset)
        return (types?.singleOrNull() as? JuxNamedElement)?.name
    }

    fun testTypeDeclarationOfALocal() {
        assertEquals(
            "Truck",
            typeDeclaration("class Truck {}\nvoid main() { Truck t = new Truck(); print(<caret>t); }"),
        )
    }

    fun testTypeDeclarationLooksThroughNullableAndArrays() {
        assertEquals("Truck", typeDeclaration("class Truck {}\nvoid main() { Truck? t = null; print(<caret>t); }"))
        assertEquals("Truck", typeDeclaration("class Truck {}\nvoid main() { Truck[] ts = new Truck[2]; print(<caret>ts); }"))
    }

    fun testTypeDeclarationOfAFieldThroughAChain() {
        assertEquals(
            "Engine",
            typeDeclaration(
                """
                class Engine {}
                class Car { public Engine engine = new Engine(); }
                void main() { var c = new Car(); print(c.eng<caret>ine); }
                """.trimIndent(),
            ),
        )
    }

    fun testTypeDeclarationOfAMethodIsItsReturnType() {
        assertEquals(
            "Engine",
            typeDeclaration(
                """
                class Engine {}
                class Car { public Engine build() { return new Engine(); } }
                void main() { var c = new Car(); c.bu<caret>ild(); }
                """.trimIndent(),
            ),
        )
    }

    // ---- Quick Definition -------------------------------------------------

    fun testQuickDefinitionShowsTheDeclaration() {
        myFixture.configureByText(
            "a.jux",
            """
            class Truck { public int load() { return 3; } }
            void main() { var t = new Truck(); print(t.lo<caret>ad()); }
            """.trimIndent(),
        )
        val shown = com.intellij.codeInsight.ShowImplementationsTestUtil.getImplementations()
        assertEquals(listOf("load"), shown.map { (it as JuxNamedElement).name })
    }

    fun testQuickDefinitionOfAnInterfaceMethodListsItsImplementations() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Shape { double area(); }
            class Sq implements Shape { public double area() { return 1.0; } }
            class Circle implements Shape { public double area() { return 3.0; } }
            void f(Shape s) { print(s.ar<caret>ea()); }
            """.trimIndent(),
        )
        val shown = com.intellij.codeInsight.ShowImplementationsTestUtil.getImplementations()
        assertTrue("the declaration and both implementations: ${shown.size}", shown.size >= 3)
    }

    fun testGoToImplementationOfAnInterface() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Sha<caret>pe { double area(); }
            class Sq implements Shape { public double area() { return 1.0; } }
            class Circle implements Shape { public double area() { return 3.0; } }
            """.trimIndent(),
        )
        val data = com.intellij.testFramework.fixtures.CodeInsightTestUtil.gotoImplementation(myFixture.editor, myFixture.file)
        assertEquals(setOf("Sq", "Circle"), data.targets.map { (it as JuxNamedElement).name }.toSet())
    }

    fun testGoToImplementationOfAMethod() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Shape { double ar<caret>ea(); }
            class Sq implements Shape { public double area() { return 1.0; } }
            """.trimIndent(),
        )
        val data = com.intellij.testFramework.fixtures.CodeInsightTestUtil.gotoImplementation(myFixture.editor, myFixture.file)
        val target = data.targets.single()
        assertEquals(E.METHOD_DECLARATION, target.elementType)
        assertEquals("Sq", (target.parent.parent as JuxNamedElement).name)
    }

    // ---- File Structure toggles -------------------------------------------

    private val shop = """
        class Base { public void inheritedOne() {} private void hidden() {} }
        class Shop extends Base {
            private int count;
            public String Name { get; set; } = "";
            public void sell() {}
            void restock() {}
        }
    """.trimIndent()

    private fun structure(active: Map<String, Boolean>): String {
        myFixture.configureByText("Shop.jux", shop)
        var out = ""
        myFixture.testStructureView { component ->
            for ((name, state) in active) component.setActionActive(name, state)
            PlatformTestUtil.expandAll(component.tree)
            out = PlatformTestUtil.print(component.tree, false)
        }
        return out
    }

    fun testFieldsFilterHidesFields() {
        val text = structure(mapOf("SHOW_FIELDS" to true))
        assertFalse(text, text.contains("count"))
        assertTrue(text, text.contains("Name"))
    }

    fun testPropertiesFilterHidesProperties() {
        val text = structure(mapOf("SHOW_PROPERTIES" to true))
        assertFalse(text, text.contains("Name"))
        assertTrue(text, text.contains("count"))
    }

    fun testNonPublicFilterHidesNonPublicMembers() {
        val text = structure(mapOf("SHOW_NON_PUBLIC" to true))
        assertFalse(text, text.contains("restock"))
        assertFalse(text, text.contains("count"))
        assertTrue(text, text.contains("sell"))
    }

    fun testShowInheritedListsSupertypeMembers() {
        fun count(text: String, name: String) = text.lines().count { it.trim() == name }
        val plain = structure(emptyMap())
        assertEquals(plain, 1, count(plain, "inheritedOne"))
        val text = structure(mapOf(InheritedMembersNodeProvider.ID to true))
        assertEquals("listed under Base and, inherited, under Shop: $text", 2, count(text, "inheritedOne"))
        assertEquals("a private member is not inherited: $text", 1, count(text, "hidden"))
    }

    fun testInheritedMemberNamesItsOwner() {
        myFixture.configureByText("Shop.jux", shop)
        val shopType = myFixture.file.children.first { (it as? JuxNamedElement)?.name == "Shop" }
        val node = dev.jux.intellij.structure.JuxStructureViewElement(shopType as com.intellij.psi.NavigatablePsiElement)
        val inherited = dev.jux.intellij.structure.JuxInheritedMembersNodeProvider().provideNodes(node)
        val labels = inherited.map { "${it.presentation.presentableText} ${it.presentation.locationString}" }
        assertEquals(listOf("inheritedOne ↑Base"), labels)
    }

    // ---- Go to Test / Create Test -----------------------------------------

    private val finder: TestFinder get() = TestFinder.EP_NAME.extensionList.first { it is JuxTestFinder }

    fun testGoToTestAndBack() {
        val source = myFixture.addFileToProject("src/shop/Cart.jux", "package shop;\npublic class Cart { public int total() { return 0; } }")
        val test = myFixture.addFileToProject(
            "test/shop/CartTest.jux",
            "package shop.test;\nimport jux.std.testing.*;\n@Test\nvoid testTotal() {}\n",
        )
        val cart = (source as JuxFile).children.first { it.elementType === E.CLASS_DECLARATION }
        val tests = finder.findTestsForClass(finder.findSourceElement(cart)!!)
        assertEquals(listOf(test), tests.map { it.containingFile })
        assertTrue(finder.isTest(test))
        assertFalse(finder.isTest(source))
        val sources = finder.findClassesForTest(test)
        assertEquals("Cart", (sources.single() as JuxNamedElement).name)
    }

    fun testCreateTestWritesTheMirroredFile() {
        val source = myFixture.addFileToProject(
            "src/shop/Cart.jux",
            "package shop;\npublic class Cart { public int total() { return 0; } public void add(int n) {} private void audit() {} }",
        ) as JuxFile
        val creator = LanguageTestCreators.INSTANCE.forLanguage(JuxLanguage)
        assertTrue("Create Test is registered for Jux", creator is JuxTestCreator)
        assertTrue(creator.isAvailable(project, null, source))
        val (file, _) = (creator as JuxTestCreator).create(project, source)!!
        assertTrue(file.path, file.path.endsWith("test/shop/CartTest.jux"))
        assertEquals(
            """
            package shop.test;

            import jux.std.testing.*;
            import shop.Cart;

            @Test
            void testTotal() {
            }

            @Test
            void testAdd() {
            }

            """.trimIndent(),
            String(file.contentsToByteArray()),
        )
        // Asked again, the existing file is reused, not overwritten.
        assertEquals(file, creator.create(project, source)!!.first)
    }
}
