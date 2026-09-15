package dev.jux.intellij.parity

import com.intellij.codeInsight.lookup.Lookup
import com.intellij.psi.PsiElement
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxNamedElement

/**
 * What IntelliJ's Java plugin does, asked of the Jux plugin.
 *
 * Each test is one everyday move a Java developer makes without thinking --
 * complete a member, jump to a method in another file, rename it everywhere,
 * import a class -- on a small project shaped like a real one: an abstract
 * base in one package, a subclass in the same package, and a `main` in the
 * root that imports both. A test that fails here is a place where writing Jux
 * feels worse than writing Java.
 */
class JuxJavaParityTest : BasePlatformTestCase() {

    private fun addVehicles() {
        myFixture.addFileToProject(
            "some/Auto.jux",
            """
            package some;

            public abstract class Auto {
                protected final int speed;
                protected final int mileage;

                protected Auto(int speed, int mileage) {
                    this.speed = speed;
                    this.mileage = mileage;
                }

                public abstract int getSpeed();

                public abstract int getMileage();

                public Auto faster(int by) { return this; }

                public static <T extends Auto> void printT(T t) {
                    print(t.getSpeed());
                }
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "some/Truck.jux",
            """
            package some;

            public final class Truck extends Auto {
                public int payload = 0;

                public Truck(int speed, int mileage) {
                    super(speed, mileage);
                }

                public int getSpeed() { return this.speed; }

                public int getMileage() { return this.mileage; }

                public void load(int tons, String cargo) { this.payload = tons; }
            }
            """.trimIndent(),
        )
    }

    private fun lookups(): List<String> = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()

    private fun resolveAt(marker: String, offsetInMarker: Int = 0): PsiElement? {
        val offset = myFixture.file.text.indexOf(marker)
        assertTrue("marker `$marker` not found", offset >= 0)
        return myFixture.file.findReferenceAt(offset + offsetInMarker)?.resolve()
    }

    private fun nameOf(e: PsiElement?): String? = (e as? JuxNamedElement)?.name

    private fun pick(name: String) {
        val items = myFixture.completeBasic() ?: return
        val item = items.firstOrNull { it.lookupString == name }
        assertNotNull("`$name` offered: ${items.map { it.lookupString }}", item)
        myFixture.lookup.currentItem = item
        myFixture.finishLookup(Lookup.NORMAL_SELECT_CHAR)
    }

    // ---------------------------------------------------------------- completion

    fun testMembersOfAnImportedClassFromAnotherPackage() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;
            import some.Auto;

            public void main() {
                Auto auto = new Truck(80, 145000);
                auto.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("getSpeed/getMileage offered: $o", o.containsAll(listOf("getSpeed", "getMileage")))
        assertFalse("a static method is not offered on an instance: $o", o.contains("printT"))
    }

    fun testInheritedMembersOnASubclassReceiver() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("own and inherited members: $o", o.containsAll(listOf("load", "payload", "faster", "getSpeed")))
    }

    fun testMembersThroughAMethodCallChain() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.faster(10).<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("members of `faster`'s return type: $o", o.contains("getMileage"))
    }

    fun testMembersOfAVarInferredFromNew() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                var t = new Truck(1, 2);
                t.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("members of the inferred type: $o", o.contains("load"))
    }

    fun testMembersOfAVarInferredFromACall() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                var quick = t.faster(2);
                quick.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("members of a var typed by a call's return type: $o", o.contains("getMileage"))
    }

    fun testMembersOfABoundedTypeParameter() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Auto;

            public <T extends Auto> void show(T t) {
                t.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("members of the bound `Auto`: $o", o.contains("getSpeed"))
    }

    fun testStaticMembersOnATypeName() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Auto;

            public void main() {
                Auto.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("static method offered: $o", o.contains("printT"))
        assertFalse("instance method not offered on the type: $o", o.contains("getSpeed"))
    }

    fun testUnimportedClassCompletesAndImportsItself() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            public void main() {
                Tru<caret>
            }
            """.trimIndent(),
        )
        pick("Truck")
        assertTrue("import added:\n${myFixture.file.text}", myFixture.file.text.contains("import some.Truck;"))
    }

    fun testMethodCompletionInsertsParenthesesWithCaretInside() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.loa<caret>
            }
            """.trimIndent(),
        )
        pick("load")
        myFixture.checkResult(
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.load(<caret>)
            }
            """.trimIndent(),
        )
    }

    fun testNewOffersProjectClassesWithImport() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            public void main() {
                var t = new Tr<caret>
            }
            """.trimIndent(),
        )
        pick("Truck")
        val text = myFixture.file.text
        assertTrue("import added:\n$text", text.contains("import some.Truck;"))
        assertTrue("constructor parentheses:\n$text", text.contains("new Truck("))
    }

    fun testStandardLibraryMembers() {
        // The shape bindgen writes for the Rust standard library (`Vec` is prelude).
        myFixture.addFileToProject(
            ".jux-stubs/std/rust-std.jux.d",
            """
            package rust.std;

            public class Vec<T, A> {
                public Vec();
                public static Self with_capacity(uint capacity);
                @MutSelf public void push(T value);
                public uint len();
                public T[] as_slice();
            }
            """.trimIndent(),
        )
        myFixture.configureByText(
            "main.jux",
            """
            public void main() {
                var v = new Vec<int>();
                v.<caret>
            }
            """.trimIndent(),
        )
        val o = lookups()
        assertTrue("Vec members from the std stubs: $o", o.containsAll(listOf("push", "len")))
    }

    // ---------------------------------------------------------------- references

    fun testGoToMethodInAnotherFile() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;
            import some.Auto;

            public void main() {
                Auto auto = new Truck(80, 145000);
                print(auto.getMileage());
            }
            """.trimIndent(),
        )
        val target = resolveAt("getMileage()")
        assertEquals("getMileage", nameOf(target))
        assertEquals("Auto.jux", target?.containingFile?.name)
    }

    fun testGoToInheritedMethodThroughSubclass() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.faster(3);
            }
            """.trimIndent(),
        )
        val target = resolveAt("faster(3)")
        assertEquals("faster", nameOf(target))
        assertEquals("Auto.jux", target?.containingFile?.name)
    }

    fun testGoToMethodOnBoundedTypeParameter() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Auto;

            public <T extends Auto> void show(T t) {
                print(t.getSpeed());
            }
            """.trimIndent(),
        )
        assertEquals("getSpeed", nameOf(resolveAt("getSpeed()")))
    }

    fun testGoToThroughAChain() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                print(t.faster(1).getMileage());
            }
            """.trimIndent(),
        )
        assertEquals("getMileage", nameOf(resolveAt("getMileage()")))
    }

    fun testGoToImportedClass() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {}
            """.trimIndent(),
        )
        assertEquals("Truck", nameOf(resolveAt("Truck;")))
    }

    fun testGoToStaticMethodOnType() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Auto;
            import some.Truck;

            public void main() {
                Auto.printT(new Truck(1, 2));
            }
            """.trimIndent(),
        )
        assertEquals("printT", nameOf(resolveAt("printT(")))
    }

    fun testGoToFieldThroughReceiver() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                print(t.payload);
            }
            """.trimIndent(),
        )
        assertEquals("payload", nameOf(resolveAt("payload)")))
    }

    fun testGoToClassFromNew() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                var t = new Truck(1, 2);
            }
            """.trimIndent(),
        )
        assertEquals("Truck", nameOf(resolveAt("Truck(1")))
    }

    fun testFindUsagesAcrossFiles() {
        addVehicles()
        myFixture.addFileToProject(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.load(1, "sand");
                t.load(2, "gravel");
            }
            """.trimIndent(),
        )
        myFixture.configureFromTempProjectFile("some/Truck.jux")
        val load = myFixture.findElementByText("void load", JuxNamedElement::class.java)
        val usages = myFixture.findUsages(load as PsiElement)
        val inMain = usages.count { it.file?.name == "main.jux" }
        assertEquals("both calls in main.jux: ${usages.map { it.file?.name }}", 2, inMain)
    }

    fun testRenameMethodAcrossFiles() {
        addVehicles()
        val main = myFixture.addFileToProject(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
                t.load(1, "sand");
            }
            """.trimIndent(),
        )
        myFixture.configureFromTempProjectFile("some/Truck.jux")
        val load = myFixture.findElementByText("void load", JuxNamedElement::class.java)
        myFixture.renameElement(load as PsiElement, "haul")
        assertTrue("call site renamed:\n${main.text}", main.text.contains("t.haul(1, \"sand\")"))
    }

    fun testRenameClassUpdatesImportsAndUses() {
        addVehicles()
        val main = myFixture.addFileToProject(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Truck t = new Truck(1, 2);
            }
            """.trimIndent(),
        )
        myFixture.configureFromTempProjectFile("some/Truck.jux")
        val truck = myFixture.findElementByText("class Truck", JuxNamedElement::class.java)
        myFixture.renameElement(truck as PsiElement, "Lorry")
        val text = main.text
        assertTrue("import renamed:\n$text", text.contains("import some.Lorry;"))
        assertTrue("uses renamed:\n$text", text.contains("Lorry t = new Lorry(1, 2);"))
    }

    // ---------------------------------------------------------------- auto-import

    fun testImportQuickFixOnUnresolvedType() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            public void main() {
                Tr<caret>uck t = null;
            }
            """.trimIndent(),
        )
        val fixes = myFixture.getAllQuickFixes().map { it.text }
        assertTrue("an import fix is offered: $fixes", fixes.any { it.contains("mport") })
    }

    fun testNoImportForASamePackageClass() {
        addVehicles()
        myFixture.configureByText(
            "Garage.jux",
            """
            package some;

            public class Garage {
                public void park() {
                    Tru<caret>
                }
            }
            """.trimIndent(),
        )
        pick("Truck")
        assertFalse("same package, no import:\n${myFixture.file.text}", myFixture.file.text.contains("import"))
        val fixes = myFixture.getAllQuickFixes().map { it.text }
        assertTrue("no import fix for a same-package class: $fixes", fixes.none { it.contains("mport") })
    }

    fun testNoSecondImportForAnAlreadyImportedClass() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.Truck;

            public void main() {
                Tru<caret>
            }
            """.trimIndent(),
        )
        pick("Truck")
        val count = Regex("""import some\.Truck;""").findAll(myFixture.file.text).count()
        assertEquals("exactly one import:\n${myFixture.file.text}", 1, count)
    }

    fun testNoImportThroughAGroupedOrWildcardImport() {
        addVehicles()
        myFixture.configureByText(
            "main.jux",
            """
            import some.{Auto, Truck};

            public void main() {
                Tru<caret>
            }
            """.trimIndent(),
        )
        pick("Truck")
        assertFalse("grouped import already covers it:\n${myFixture.file.text}", myFixture.file.text.contains("import some.Truck;"))
        myFixture.configureByText(
            "wild.jux",
            """
            import some.*;

            public void main() {
                Tru<caret>
            }
            """.trimIndent(),
        )
        pick("Truck")
        assertFalse("wildcard import already covers it:\n${myFixture.file.text}", myFixture.file.text.contains("import some.Truck;"))
    }

    fun testNoSelfImport() {
        myFixture.configureByText(
            "Solo.jux",
            """
            package some;

            public class Solo {
                public Solo copy() {
                    return new So<caret>
                }
            }
            """.trimIndent(),
        )
        pick("Solo")
        assertFalse("a class never imports itself:\n${myFixture.file.text}", myFixture.file.text.contains("import"))
    }

    // ---------------------------------------------------------------- documentation

    fun testDocCommentStubListsParametersReturnAndThrows() {
        myFixture.configureByText(
            "Calc.jux",
            """
            public class Calc {
                /**<caret>
                public int add(int a, int b) throws IllegalStateException { return a + b; }
            }
            """.trimIndent(),
        )
        myFixture.type('\n')
        val text = myFixture.file.text
        assertTrue("@param a:\n$text", text.contains("@param a"))
        assertTrue("@param b:\n$text", text.contains("@param b"))
        assertTrue("@return:\n$text", text.contains("@return"))
        assertTrue("@throws:\n$text", text.contains("@throws IllegalStateException"))
    }
}
