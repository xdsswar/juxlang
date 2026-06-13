package dev.jux.intellij.intentions

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxImportTypeIntention] — the "Import 'pkg.Type'" Alt+Enter action. Verifies
 * it appears (and inserts the right import) for an unqualified cross-package
 * type, and stays out of the way for same-package / already-imported / in-file
 * types.
 */
class JuxImportTypeIntentionTest : BasePlatformTestCase() {

    private fun addWidget(pkg: String = "lib") {
        myFixture.addFileToProject("$pkg/Widget.jux", "package $pkg;\npublic class Widget {}\n")
    }

    fun testImportsCrossPackageType() {
        addWidget("lib")
        myFixture.configureByText(
            "App.jux",
            """
            package app;
            public class App {
                public void go(Widget<caret> w) {}
            }
            """.trimIndent(),
        )
        val intention = myFixture.findSingleIntention("Import 'lib.Widget'")
        myFixture.launchAction(intention)
        assertTrue(
            "import should be inserted: ${myFixture.file.text}",
            myFixture.file.text.contains("import lib.Widget;"),
        )
    }

    fun testNotOfferedForSamePackageType() {
        addWidget("app") // same package as the using file
        myFixture.configureByText(
            "App.jux",
            """
            package app;
            public class App {
                public void go(Widget<caret> w) {}
            }
            """.trimIndent(),
        )
        assertEmpty(myFixture.filterAvailableIntentions("Import 'app.Widget'"))
    }

    fun testNotOfferedWhenAlreadyImported() {
        addWidget("lib")
        myFixture.configureByText(
            "App.jux",
            """
            package app;
            import lib.Widget;
            public class App {
                public void go(Widget<caret> w) {}
            }
            """.trimIndent(),
        )
        assertEmpty(myFixture.filterAvailableIntentions("Import 'lib.Widget'"))
    }

    fun testNotOfferedForInFileType() {
        myFixture.configureByText(
            "App.jux",
            """
            package app;
            public class Widget {}
            public class App {
                public void go(Widget<caret> w) {}
            }
            """.trimIndent(),
        )
        assertEmpty(myFixture.filterAvailableIntentions("Import"))
    }
}
