package dev.jux.intellij.refactoring

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.refactoring.JuxChangeSignature.Parameter as P

/**
 * Change Signature (`Ctrl+F6`): the declaration, its override family, every
 * call, and a renamed parameter's uses all change together.
 */
class JuxChangeSignatureTest : BasePlatformTestCase() {

    fun testRenameReorderAddAndRemoveAcrossFiles() {
        val shop = myFixture.addFileToProject(
            "Shop.jux",
            """
            public class Shop {
                public static int sell(String item, int qty, bool gift) {
                    print(${'$'}"sold ${'$'}{qty} of ${'$'}item");
                    return qty;
                }
            }
            """.trimIndent(),
        )
        val app = myFixture.addFileToProject(
            "App.jux",
            """
            void main() {
                var n = Shop.sell("tea", 2, false);
                print(n);
            }
            """.trimIndent(),
        )
        val sell = method(shop, "sell")
        JuxChangeSignature(
            sell, "buy", "long",
            listOf(P(1, "count", "int"), P(0, "item", "String"), P(-1, "price", "double", "1.5")),
        ).run(project)

        assertEquals(
            """
            public class Shop {
                public static long buy(int count, String item, double price) {
                    print(${'$'}"sold ${'$'}{count} of ${'$'}item");
                    return count;
                }
            }
            """.trimIndent(),
            shop.text,
        )
        assertEquals(
            """
            void main() {
                var n = Shop.buy(2, "tea", 1.5);
                print(n);
            }
            """.trimIndent(),
            app.text,
        )
    }

    fun testOverridesChangeWithTheMethod() {
        val file = myFixture.addFileToProject(
            "Animals.jux",
            """
            abstract class Animal {
                abstract String speak(int times);
            }
            class Dog extends Animal {
                @Override
                String speak(int times) { return "woof"; }
            }
            void main() {
                Animal a = new Dog();
                print(a.speak(2));
            }
            """.trimIndent(),
        )
        val base = method(file, "speak")
        JuxChangeSignature(base, "talk", null, listOf(P(0, "times", "int"), P(-1, "loud", "bool", "false"))).run(project)
        val text = file.text
        assertTrue(text, text.contains("abstract String talk(int times, bool loud);"))
        assertTrue(text, text.contains("String talk(int times, bool loud) { return \"woof\"; }"))
        assertTrue(text, text.contains("print(a.talk(2, false));"))
    }

    fun testARecursiveCallKeepsTheRenameInsideItsReorderedArguments() {
        val file = myFixture.addFileToProject(
            "Rec.jux",
            """
            int count(int n, int step) {
                if (n <= 0) {
                    return 0;
                }
                return 1 + count(n - step, step);
            }
            """.trimIndent(),
        )
        JuxChangeSignature(method(file, "count"), "count", null, listOf(P(1, "by", "int"), P(0, "left", "int"))).run(project)
        assertEquals(
            """
            int count(int by, int left) {
                if (left <= 0) {
                    return 0;
                }
                return 1 + count(by, left - by);
            }
            """.trimIndent(),
            file.text,
        )
    }

    fun testSwappingTwoNamesDoesNotChainInsideAnInterpolation() {
        val file = myFixture.addFileToProject(
            "Swap.jux",
            """
            void show(int a, int b) {
                print(${'$'}"${'$'}a then ${'$'}{b}");
            }
            """.trimIndent(),
        )
        JuxChangeSignature(method(file, "show"), "show", null, listOf(P(0, "b", "int"), P(1, "a", "int"))).run(project)
        assertEquals(
            """
            void show(int b, int a) {
                print(${'$'}"${'$'}b then ${'$'}{a}");
            }
            """.trimIndent(),
            file.text,
        )
    }

    fun testANewParameterWithoutAValueIsAProblem() {
        val file = myFixture.addFileToProject("A.jux", "void f(int a) {}")
        val change = JuxChangeSignature(method(file, "f"), "f", null, listOf(P(0, "a", "int"), P(-1, "b", "int")))
        assertTrue(change.problems().toString(), change.problems().any { it.contains("value to pass") })
    }

    fun testTheHandlerIsRegistered() {
        val provider = com.intellij.lang.LanguageRefactoringSupport.getInstance().forLanguage(dev.jux.intellij.JuxLanguage)
        assertTrue(provider.changeSignatureHandler is JuxChangeSignatureHandler)
        assertTrue(provider.extractMethodHandler is JuxExtractMethodHandler)
    }

    private fun method(file: com.intellij.psi.PsiFile, name: String): PsiElement =
        PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java).first { it.name == name }
}
