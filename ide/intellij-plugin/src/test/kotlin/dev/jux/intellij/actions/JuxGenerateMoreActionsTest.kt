package dev.jux.intellij.actions

import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The Generate actions beyond the basics: the field chooser, Copy
 * Constructor, Delegate Methods and the test-function generators.
 */
class JuxGenerateMoreActionsTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxMemberChooser.testChoice = null
        } finally {
            super.tearDown()
        }
    }

    private fun text() = myFixture.editor.document.text

    fun testTheChooserPicksTheFields() {
        myFixture.configureByText(
            "Person.jux",
            """
            public class Person {
                private String name;
                private int age;
                private double score;
                <caret>
            }
            """.trimIndent(),
        )
        // The user unticks `score`.
        JuxMemberChooser.testChoice = { offered -> offered.filter { (it as JuxField).name != "score" } }
        myFixture.testAction(JuxGenerateConstructorAction())
        assertTrue(text(), text().contains("public Person(String name, int age) {"))
        assertFalse(text(), text().contains("this.score"))
    }

    fun testCopyConstructorCopiesEveryStoredField() {
        myFixture.configureByText(
            "Box.jux",
            """
            public class Box<T> {
                private T item;
                private int count;
                public String Label -> "box";
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateCopyConstructorAction())
        assertTrue(text(), text().contains("public Box(Box<T> other) {"))
        assertTrue(text(), text().contains("this.item = other.item;"))
        assertTrue(text(), text().contains("this.count = other.count;"))
        assertFalse("a computed property has nothing to copy", text().contains("this.Label"))
    }

    fun testDelegateMethodsForwardToTheField() {
        myFixture.configureByText(
            "Shop.jux",
            """
            public class Stock<T> {
                public T take(int index) { return null; }
                public void put(T item) { }
                public static int limit() { return 3; }
            }
            public class Shop {
                private Stock<String> stock;
                private int id;
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateDelegateMethodsAction())
        // The generic argument is substituted; statics are not offered.
        assertTrue(text(), text().contains("public String take(int index) {\n        return stock.take(index);\n    }"))
        assertTrue(text(), text().contains("public void put(String item) {\n        stock.put(item);\n    }"))
        assertFalse(text(), text().contains("stock.limit()"))
    }

    fun testDelegateMethodsNeedAFieldOfAProjectType() {
        myFixture.configureByText("A.jux", "public class A { private int n; <caret> }")
        myFixture.testAction(JuxGenerateDelegateMethodsAction())
        // `int` is not a project type: there is nothing to delegate to, and
        // the file is left alone.
        assertEquals("public class A { private int n;  }", text())
    }

    fun testTestFunctionAtTheTopOfAFile() {
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        myFixture.configureByText("t.jux", "void testName() { }\n<caret>\n")
        myFixture.testAction(JuxGenerateTestFunctionAction())
        WriteCommandAction.runWriteCommandAction(project) {
            TemplateManagerImpl.getTemplateState(myFixture.editor)?.gotoEnd(false)
        }
        // The name is free: `testName` is taken, so it counts on.
        assertTrue(text(), text().contains("@Test\nvoid testName2() {"))
    }

    fun testTestGeneratorsStayOutOfTypes() {
        myFixture.configureByText("t.jux", "class A { <caret> }")
        assertFalse(myFixture.testAction(JuxGenerateTestFunctionAction()).isEnabledAndVisible)
    }

    fun testAHookIsOfferedOncePerFile() {
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        myFixture.configureByText("t.jux", "@beforeEach\nvoid setUp() { }\n<caret>\n")
        assertFalse(myFixture.testAction(JuxGenerateBeforeEachAction()).isEnabledAndVisible)
        assertTrue(myFixture.testAction(JuxGenerateAfterEachAction()).isEnabledAndVisible)
    }
}
