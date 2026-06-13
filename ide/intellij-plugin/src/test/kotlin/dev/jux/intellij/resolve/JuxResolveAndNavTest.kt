package dev.jux.intellij.resolve

import com.intellij.psi.search.GlobalSearchScope
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.indexing.FindSymbolParameters
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Cross-file resolution, soft references, Go-to contributors, and the
 * override/implement gutter — the resolve-layer additions of the polish wave.
 */
class JuxResolveAndNavTest : BasePlatformTestCase() {

    fun testReferenceIsSoft() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A { public void go() { unknownThing(); } }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("unknownThing")
        val ref = myFixture.file.findReferenceAt(offset)
        assertNotNull("identifier should carry a reference", ref)
        assertTrue("the in-file resolver must be soft", ref!!.isSoft)
    }

    fun testCrossFileTypeResolution() {
        myFixture.addFileToProject("beast.jux", "package demo;\npublic class Beast {}\n")
        myFixture.configureByText(
            "user.jux",
            """
            package demo;
            public class User { public void pet(Beast b) {} }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("Beast")
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("Beast should resolve cross-file via JuxTypeIndex", resolved)
        assertEquals("Beast", (resolved as JuxTypeDeclaration).name)
    }

    fun testGotoClassContributorFindsTypes() {
        myFixture.addFileToProject("beast.jux", "package demo;\npublic class Beast {}\n")
        myFixture.configureByText("other.jux", "package demo;\npublic class Other {}\n")

        val names = ArrayList<String>()
        JuxGotoClassContributor().processNames(
            { names.add(it) },
            GlobalSearchScope.projectScope(project),
            null,
        )
        assertContainsElements(names, "Beast", "Other")

        val items = ArrayList<Any>()
        JuxGotoClassContributor().processElementsWithName(
            "Beast",
            { items.add(it) },
            FindSymbolParameters.simple(project, false),
        )
        assertSize(1, items)
    }

    fun testGotoSymbolContributorFindsMembers() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Box { public int size; public void open() {} }
            """.trimIndent(),
        )
        val names = ArrayList<String>()
        JuxGotoSymbolContributor().processNames(
            { names.add(it) },
            GlobalSearchScope.projectScope(project),
            null,
        )
        assertContainsElements(names, "Box", "size", "open")
    }

    fun testOverrideGutterAppearsOnOverridingMethod() {
        myFixture.addFileToProject(
            "animal.jux",
            """
            package demo;
            public class Animal { public void speak() {} }
            """.trimIndent(),
        )
        myFixture.configureByText(
            "dog.jux",
            """
            package demo;
            public class Dog extends Animal {
                @override
                public void spea<caret>k() {}
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findGuttersAtCaret()
        assertTrue(
            "expected an overrides/implements gutter on Dog.speak",
            gutters.any { it.tooltipText?.contains("Animal") == true },
        )
    }

    // ---- member Go-to (resolve through the receiver's type) -----------------

    fun testMethodAccessResolvesThroughParamType() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Engine { public void start() {} }
            public class Car {
                public void go(Engine e) {
                    e.start();
                }
            }
            """.trimIndent(),
        )
        // The call site `e.start();` (the declaration is `start() {}`, no `;`).
        val offset = myFixture.file.text.indexOf("start();")
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("e.start() should resolve to Engine.start", resolved)
        assertEquals("start", (resolved as dev.jux.intellij.psi.JuxNamedElement).name)
    }

    fun testFieldAccessResolvesThroughNewLocalType() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Point { public int x; public int y; }
            public class App {
                public void go() {
                    var p = new Point();
                    var z = p.x;
                }
            }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("p.x") + 2 // land on `x`
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("p.x should resolve to Point.x", resolved)
        assertEquals("x", (resolved as dev.jux.intellij.psi.JuxNamedElement).name)
    }

    fun testThisMemberResolvesToEnclosingMember() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Widget {
                private int width;
                public void resize() { var w = this.width; }
            }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("this.width") + 5 // land on `width`
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("this.width should resolve to the field", resolved)
        assertEquals("width", (resolved as dev.jux.intellij.psi.JuxNamedElement).name)
    }

    fun testInheritedMemberAccessResolves() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Base { public void shared() {} }
            public class Derived extends Base {}
            public class App {
                public void go(Derived d) {
                    d.shared();
                }
            }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("shared();")
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("d.shared() should resolve to inherited Base.shared", resolved)
        assertEquals("shared", (resolved as dev.jux.intellij.psi.JuxNamedElement).name)
    }

    // ---- §P.7.8 property gutter trio ----------------------------------------

    fun testPropertyGutterTrioStatuses() {
        // Watcher attaches to Model.Name and binds Model.Title cross-file —
        // the slow pass aggregates every project file's usage scan.
        myFixture.addFileToProject(
            "watcher.jux",
            """
            package demo;
            public class Watcher {
                public String Local { get; set; } = "";
                private final observer<String> obs = (old, now) -> { print(now); };
                public Watcher(Model m) {
                    m.Name.observers.attach(obs);
                    Local.bind(m.Title);
                }
            }
            """.trimIndent(),
        )
        myFixture.configureByText(
            "model.jux",
            """
            package demo;
            public class Model {
                public String Name { get; set; } = "";
                public String Title { get; set; } = "";
                public String Untouched { get; set; } = "";
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findAllGutters().mapNotNull { it.tooltipText }

        assertTrue(
            "Name is observed (cross-file attach): $gutters",
            gutters.any { it.contains("'Name' is observed") },
        )
        assertTrue(
            "Title is a binding source → bound: $gutters",
            gutters.any { it.contains("'Title' is bound") },
        )
        assertTrue(
            "Untouched gets the plain marker: $gutters",
            gutters.any { it.contains("'Untouched' is not observed or bound") },
        )
    }

    fun testPropertyGutterBoundReceiver() {
        myFixture.configureByText(
            "form.jux",
            """
            package demo;
            public class Form {
                public String Label { get; set; } = "";
                public String Source { get; set; } = "";
                public void wire() {
                    Label.bind(Source);
                }
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findAllGutters().mapNotNull { it.tooltipText }
        assertTrue("bind receiver is bound: $gutters", gutters.any { it.contains("'Label' is bound") })
        assertTrue("bind argument is a source → bound: $gutters", gutters.any { it.contains("'Source' is bound") })
    }
}
