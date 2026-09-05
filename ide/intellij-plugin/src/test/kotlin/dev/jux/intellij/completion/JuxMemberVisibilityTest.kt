package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * What the fallback offers as a member, and to whom.
 *
 * Two asymmetries it had. It offered every member of a receiver regardless of
 * visibility, so `other.` listed another class's `private` fields — names that
 * do not compile. And the bare-identifier path read only the enclosing class's
 * own body, so an INHERITED member completed after `this.` but not on its own,
 * even though both are the same reference.
 *
 * The language server gets both right (`intel::member_visible`), which is the
 * point: the fallback should differ from it in depth, never in what is legal.
 */
class JuxMemberVisibilityTest : BasePlatformTestCase() {

    private fun offered(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testPrivateMembersOfAnotherClassAreNotOffered() {
        val o = offered(
            """
            public class Secret {
                private int hidden = 1;
                private int tell() { return 1; }
                public int shown = 2;
                public int show() { return 2; }
            }
            void main() { Secret s = new Secret(); s.<caret> }
            """.trimIndent(),
        )
        assertTrue("public members are offered: $o", o.containsAll(listOf("shown", "show")))
        assertFalse("a private field is not reachable here: $o", o.contains("hidden"))
        assertFalse("a private method is not reachable here: $o", o.contains("tell"))
    }

    fun testPrivateMembersOfTheOwnClassAreOffered() {
        val o = offered(
            """
            public class Secret {
                private int hidden = 1;
                public int use() { Secret s = new Secret(); s.<caret> return 0; }
            }
            """.trimIndent(),
        )
        assertTrue("a class sees its own privates: $o", o.contains("hidden"))
    }

    fun testProtectedMembersAreOfferedToASubclass() {
        val o = offered(
            """
            public class Base { protected int shared = 1; private int mine = 2; }
            public class Derived extends Base {
                public int use() { Derived d = new Derived(); d.<caret> return 0; }
            }
            """.trimIndent(),
        )
        assertTrue("protected reaches the subclass: $o", o.contains("shared"))
        assertFalse("private does not: $o", o.contains("mine"))
    }

    fun testInheritedMembersCompleteAsBareNames() {
        val o = offered(
            """
            public class Base { public int shared = 1; public int helper() { return 1; } }
            public class Derived extends Base {
                public int use() { return <caret> }
            }
            """.trimIndent(),
        )
        assertTrue("an inherited field is in scope unqualified: $o", o.contains("shared"))
        assertTrue("so is an inherited method: $o", o.contains("helper"))
    }
}
