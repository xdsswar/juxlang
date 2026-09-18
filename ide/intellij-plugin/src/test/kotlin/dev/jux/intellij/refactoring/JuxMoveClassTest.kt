package dev.jux.intellij.refactoring

import com.intellij.psi.PsiManager
import com.intellij.refactoring.move.MoveHandlerDelegate
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxTypeDeclaration

/** Move Class (`F6`): the file, its package line, and every user's imports. */
class JuxMoveClassTest : BasePlatformTestCase() {

    fun testMovingAFileUpdatesItsPackageAndEveryUser() {
        val truck = myFixture.addFileToProject(
            "fleet/Truck.jux",
            """
            package fleet;

            public class Truck {
                public Wheel front = new Wheel();
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "fleet/Wheel.jux",
            """
            package fleet;

            public class Wheel {}
            """.trimIndent(),
        )
        val garage = myFixture.addFileToProject(
            "fleet/Garage.jux",
            """
            package fleet;

            public class Garage {
                public Truck parked = new Truck();
            }
            """.trimIndent(),
        )
        val app = myFixture.addFileToProject(
            "app/App.jux",
            """
            package app;

            import fleet.Truck;

            void main() {
                var t = new Truck();
            }
            """.trimIndent(),
        )
        val type = truck.children.filterIsInstance<JuxTypeDeclaration>().first()
        JuxMoveClass(type, "fleet.heavy").run(project)

        val moved = myFixture.findFileInTempDir("fleet/heavy/Truck.jux")
        assertNotNull("the file moves to the package's directory", moved)
        assertEquals(
            """
            package fleet.heavy;

            import fleet.Wheel;

            public class Truck {
                public Wheel front = new Wheel();
            }
            """.trimIndent(),
            PsiManager.getInstance(project).findFile(moved!!)!!.text,
        )
        assertEquals(
            """
            package fleet;

            import fleet.heavy.Truck;

            public class Garage {
                public Truck parked = new Truck();
            }
            """.trimIndent(),
            garage.text,
        )
        assertTrue(app.text, app.text.contains("import fleet.heavy.Truck;"))
        assertFalse(app.text, app.text.contains("import fleet.Truck;"))
    }

    fun testATypeSharingAFileIsSplitOut() {
        val file = myFixture.addFileToProject(
            "shapes/Shapes.jux",
            """
            package shapes;

            public class Circle {}

            class Square {}
            """.trimIndent(),
        )
        val square = file.children.filterIsInstance<JuxTypeDeclaration>().first { it.name == "Square" }
        JuxMoveClass(square, "shapes.flat").run(project)
        assertEquals("package shapes;\n\npublic class Circle {}\n", file.text.let { if (it.endsWith("\n")) it else "$it\n" })
        val created = myFixture.findFileInTempDir("shapes/flat/Square.jux")
        assertNotNull(created)
        assertEquals("package shapes.flat;\n\nclass Square {}\n", PsiManager.getInstance(project).findFile(created!!)!!.text)
    }

    fun testMovingToItsOwnPackageIsAProblem() {
        val file = myFixture.addFileToProject("p/A.jux", "package p;\n\npublic class A {}\n")
        val type = file.children.filterIsInstance<JuxTypeDeclaration>().first()
        assertTrue(JuxMoveClass(type, "p").problems().any { it.contains("already") })
    }

    fun testTheHandlerIsRegistered() {
        assertTrue(MoveHandlerDelegate.EP_NAME.extensionList.any { it is JuxMoveClassHandler })
    }
}
