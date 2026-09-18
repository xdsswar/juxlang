package dev.jux.intellij.structure

import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The Structure view (Ctrl+F12 / the Structure tool window): types, members,
 * nested types and properties, with parameters and locals left out.
 */
class JuxStructureViewTest : BasePlatformTestCase() {

    private fun assertStructure(text: String, expected: String) {
        myFixture.configureByText("Shop.jux", text)
        myFixture.testStructureView { component ->
            PlatformTestUtil.expandAll(component.tree)
            PlatformTestUtil.assertTreeEqual(component.tree, expected.trimIndent())
        }
    }

    fun testMembersNestedTypesAndProperties() = assertStructure(
        """
        public class Shop {
            private int count;
            public String Name { get; set; } = "";
            public Shop() {}
            public void sell(int qty) {
                int local = qty;
            }
            public bool operator==(Shop other) { return true; }
            public enum Kind { Retail, Online }
            public record Line(int qty) {}
        }
        public interface Priced {
            double price();
        }
        void main() {}
        """.trimIndent(),
        """
        -Shop.jux
         -Shop
          count
          Name
          Shop
          sell
          operator==
          -Kind
           Retail
           Online
          Line
         -Priced
          price
         main
        """,
    )

    fun testEmptyFile() = assertStructure("", "Shop.jux")
}
