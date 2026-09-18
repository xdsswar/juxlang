package dev.jux.intellij.quickfix

import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxAbstractNotImplementedInspection
import dev.jux.intellij.inspections.JuxUnresolvedReferenceInspection

/**
 * Java's "Create … from usage" family and the modifier fixes: what each
 * writes, where it puts it, and when it is offered.
 */
class JuxCreateFromUsageTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        myFixture.enableInspections(JuxUnresolvedReferenceInspection(), JuxAbstractNotImplementedInspection())
    }

    /** Runs the fix named [fix] at the caret (or anywhere in the file) and accepts every template default. */
    private fun apply(fix: String) {
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: myFixture.availableIntentions.firstOrNull { it.text == fix }
            ?: error("no '$fix' among ${myFixture.getAllQuickFixes().map { it.text } + myFixture.availableIntentions.map { it.text }}")
        myFixture.launchAction(action)
        finishTemplate()
    }

    private fun finishTemplate() {
        WriteCommandAction.runWriteCommandAction(project) {
            TemplateManagerImpl.getTemplateState(myFixture.editor)?.gotoEnd(false)
        }
    }

    private fun text(): String = myFixture.editor.document.text

    // ---- types ------------------------------------------------------------

    fun testCreateClassInNewFileWithPackageAndConstructor() {
        val main = myFixture.addFileToProject("demo/Main.jux", "package demo;\n\nvoid main() {\n    var w = new Widget(5, \"a\");\n}\n")
        myFixture.configureFromExistingVirtualFile(main.virtualFile)
        myFixture.doHighlighting()
        val fix = myFixture.getAllQuickFixes().first { it.text == "Create class 'Widget'" }
        myFixture.launchAction(fix)
        val created = main.containingDirectory.findFile("Widget.jux") ?: error("Widget.jux not created")
        val text = created.text
        assertTrue(text, text.startsWith("package demo;"))
        assertTrue(text, text.contains("public class Widget {"))
        assertTrue(text, text.contains("public Widget(int i, String s)"))
    }

    fun testCreateTypeKindsFollowTheClause() {
        myFixture.configureByText("a.jux", "public class A implements Shape {}\n")
        myFixture.doHighlighting()
        val names = myFixture.getAllQuickFixes().map { it.text }
        assertTrue(names.toString(), "Create interface 'Shape'" in names)
        assertFalse(names.toString(), "Create class 'Shape'" in names)
    }

    fun testCreateInnerClass() {
        myFixture.configureByText("a.jux", "public class A {\n    private Part part;\n}\n")
        apply("Create inner class 'Part'")
        assertTrue(text(), text().contains("class Part {"))
        assertTrue(text(), text().indexOf("class Part") > text().indexOf("private Part part;"))
    }

    // ---- methods ----------------------------------------------------------

    fun testCreateMethodFromBareCall() {
        myFixture.configureByText(
            "a.jux",
            "public class Shop {\n    public void run(String name) {\n        int n = compute(3, name);\n    }\n}\n",
        )
        apply("Create method 'compute'")
        val t = text()
        assertTrue(t, t.contains("private int compute(int i, String name) {"))
        assertTrue(t, t.contains("throw new UnsupportedOperationException();"))
        // After the method that calls it, still inside the class.
        assertTrue(t, t.indexOf("compute(int i") > t.indexOf("public void run"))
        assertTrue(t, t.trimEnd().endsWith("}"))
    }

    fun testCreateFreeFunctionFromBareCall() {
        myFixture.configureByText("a.jux", "void main() {\n    greet();\n}\n")
        apply("Create method 'greet'")
        assertTrue(text(), text().contains("void greet() {"))
        assertFalse(text(), text().contains("private"))
    }

    fun testCreateMethodInReceiversClass() {
        myFixture.configureByText(
            "a.jux",
            "public class Order {\n}\n\nvoid main() {\n    var o = new Order();\n    o.sh<caret>ip(3);\n}\n",
        )
        val intention = myFixture.findSingleIntention("Create method 'ship' in 'Order'")
        myFixture.launchAction(intention)
        finishTemplate()
        val t = text()
        assertTrue(t, t.contains("public void ship(int i) {"))
        assertTrue(t, t.indexOf("ship(int i)") < t.indexOf("void main()"))
    }

    fun testNoCreateMethodWhenItExists() {
        myFixture.configureByText(
            "a.jux",
            "public class Order {\n    public void ship(int n) {}\n}\n\nvoid main() {\n    var o = new Order();\n    o.sh<caret>ip(3);\n}\n",
        )
        assertTrue(myFixture.filterAvailableIntentions("Create method 'ship' in 'Order'").isEmpty())
    }

    // ---- variables --------------------------------------------------------

    fun testCreateLocalFromAssignment() {
        myFixture.configureByText("a.jux", "void main() {\n    total = 5;\n}\n")
        apply("Create local variable 'total'")
        assertTrue(text(), text().contains("int total = 5;"))
    }

    fun testCreateLocalAboveTheStatement() {
        myFixture.configureByText("a.jux", "void main() {\n    int y = count + 1;\n}\n")
        apply("Create local variable 'count'")
        assertTrue(text(), text().contains("    int count = 0;\n    int y = count + 1;"))
    }

    fun testCreateField() {
        myFixture.configureByText("a.jux", "public class A {\n    private int x = 0;\n\n    public int f() {\n        return x + limit;\n    }\n}\n")
        apply("Create field 'limit'")
        assertTrue(text(), text().contains("    private int x = 0;\n    private int limit = 0;"))
    }

    fun testCreateParameter() {
        myFixture.configureByText("a.jux", "int f(int a) {\n    return a * scale;\n}\n")
        apply("Create parameter 'scale'")
        assertTrue(text(), text().contains("int f(int a, int scale)"))
    }

    // ---- make abstract ----------------------------------------------------

    fun testMakeClassAbstractInsteadOfImplementing() {
        myFixture.configureByText("a.jux", "interface Shape {\n    double area();\n}\n\npublic class Square implements Shape {\n}\n")
        apply("Make 'Square' abstract")
        assertTrue(text(), text().contains("public abstract class Square implements Shape"))
    }
}
