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

    fun testKeywordMemberCarriesReference() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void go(Obj o) { o.default(); }
            }
            """.trimIndent(),
        )
        // A keyword in member position (`o.default()`) is a member name, so it
        // carries a reference (context-aware) — powering go-to / completion on
        // Rust crate members whose names collide with Jux keywords.
        val offset = myFixture.file.text.indexOf("default")
        assertNotNull("keyword member should carry a reference", myFixture.file.findReferenceAt(offset))
    }

    fun testNativeFnCallResolves() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            @extern(lib = "c")
            unsafe native {
                i32 puts(String s);
            }
            public void main() {
                unsafe { puts("hi"); }
            }
            """.trimIndent(),
        )
        // The call site `puts("hi")` resolves into the native block's foreign fn.
        val offset = myFixture.file.text.indexOf("puts(\"hi\")")
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("native fn call should resolve into the extern block", resolved)
        assertEquals("puts", (resolved as dev.jux.intellij.psi.JuxNamedElement).name)
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

    // ---- reverse (down-arrow) subtype / override gutters --------------------

    fun testIsSubclassedAndOverriddenGuttersOnSuperclass() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Shape { public double area() { return 0.0; } }
            public class Circle extends Shape { public double area() { return 1.0; } }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findAllGutters().mapNotNull { it.tooltipText }
        assertTrue("Shape is subclassed by Circle: $gutters", gutters.any { it.contains("subclassed by", true) })
        assertTrue("Shape.area is overridden in Circle: $gutters", gutters.any { it.contains("overridden in", true) })
    }

    fun testIsImplementedGuttersOnInterface() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public interface Speaker { void speak(); }
            public class Dog implements Speaker { public void speak() {} }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findAllGutters().mapNotNull { it.tooltipText }
        assertTrue("Speaker is implemented by Dog: $gutters", gutters.any { it.contains("implemented by", true) })
        assertTrue("speak() is implemented in Dog: $gutters", gutters.any { it.contains("implemented in", true) })
    }

    fun testNoReverseGutterWithoutSubtypes() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Lonely { public void solo() {} }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findAllGutters().mapNotNull { it.tooltipText }
        assertFalse("no down-markers when nothing extends it: $gutters", gutters.any { it.contains("subclassed", true) })
        assertFalse(gutters.any { it.contains("overridden in", true) })
    }

    fun testChainedMemberAccessDefersInsteadOfMisResolving() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Wrong { public int v; }
            public class Right { public int v; }
            public class Mid { public Right leaf; }
            public class App {
                public Wrong leaf;
                public void go(Mid m) {
                    var z = m.leaf.v;
                }
            }
            """.trimIndent(),
        )
        // `m.leaf.v` is a chained receiver (`m.leaf`). The in-file resolver must
        // DEFER (null) rather than mis-resolve `v` via App's same-named `leaf`
        // field (type Wrong) — that would jump to the wrong `v`.
        val offset = myFixture.file.text.indexOf(".v;") + 1
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNull("chained member must defer, not mis-resolve to Wrong.v", resolved)
    }

    fun testStaticMethodIsNotAnOverride() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Base { public void f() {} }
            public class Sub extends Base { public static void f() {} }
            """.trimIndent(),
        )
        val f = com.intellij.psi.util.PsiTreeUtil
            .findChildrenOfType(myFixture.file, dev.jux.intellij.psi.JuxMethodDeclaration::class.java)
            .first { it.name == "f" && JuxHierarchy.enclosingType(it)?.name == "Base" }
        // Sub.f is static — a same-signature static method is not an override.
        assertEmpty(JuxSubtypes.overridingMethods(f))
    }

    // ---- Go To Implementation (Ctrl+Alt+B) ----------------------------------

    fun testGoToImplementationFindsSubtypesAndOverrides() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Shape { public double area() { return 0.0; } }
            public class Circle extends Shape { public double area() { return 1.0; } }
            public class Square extends Shape { public double area() { return 2.0; } }
            """.trimIndent(),
        )
        // Subtypes of Shape (the declaration), via the shared reverse index.
        val shape = com.intellij.psi.util.PsiTreeUtil
            .findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java)
            .first { it.name == "Shape" }
        val subs = JuxSubtypes.subtypesOf(shape).mapNotNull { it.name }.toSet()
        assertEquals(setOf("Circle", "Square"), subs)

        // Overrides of Shape.area().
        val area = com.intellij.psi.util.PsiTreeUtil
            .findChildrenOfType(myFixture.file, dev.jux.intellij.psi.JuxMethodDeclaration::class.java)
            .first { it.name == "area" && JuxHierarchy.enclosingType(it)?.name == "Shape" }
        val overrides = JuxSubtypes.overridingMethods(area)
            .mapNotNull { JuxHierarchy.enclosingType(it)?.name }.toSet()
        assertEquals(setOf("Circle", "Square"), overrides)
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
