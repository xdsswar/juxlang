package dev.jux.intellij.resolve

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxLocalVariable

/**
 * Two things the compiler learned that the editor had not: the positional
 * pattern over a sealed class's permitted subclasses (ERRATA E101,
 * `JUX-GRAMMAR-ADDENDUM.md` §A.3) and the member surface reached through a
 * wildcard over a class bound (ERRATA E100,
 * `JUX-TYPE-SYSTEM-ADDENDUM.md` §T.4.8).
 *
 * Both used to type as nothing in the editor, which is quiet but useless: no
 * completion after the dot, no go-to on the member, no parameter info, and a
 * guard (`case Red(var s, _) when s > 20`) typed against an unknown.
 */
class JuxSubclassPatternAndWildcardTest : BasePlatformTestCase() {

    /** The `sealed class Light permits Red, Yellow, Green` of `examples/sealed_subclass_patterns.jux`. */
    private val lights = """
        sealed class Light permits Red, Yellow, Green {}

        final class Red extends Light {
            public int seconds;
            public String note;
            public Red(int s, String note) { this.seconds = s; this.note = note; }
        }

        final class Yellow extends Light {
            public static int LIMIT = 9;
            public int seconds;
            public bool flashing;
            public Yellow(int s, bool f) { this.seconds = s; this.flashing = f; }
        }

        final class Green extends Light {
            public int seconds;
            public bool arrow;
            public Green(int s, bool arrow) { this.seconds = s; this.arrow = arrow; }
        }
    """.trimIndent()

    /**
     * `class Animal` / `class Dog` of `examples/wildcard_bound_members.jux`,
     * plus a container declared IN the fixture.
     *
     * The example itself writes `Vec<? extends Animal>`, and `Vec` is a
     * `rust.std` stub type whose source the plugin indexes from the toolchain's
     * stub cache -- which exists on a machine that has built the corpus and not
     * on one that has not. A container the fixture declares makes the wildcard
     * rule the only variable in these assertions, which is what they are about;
     * the `Vec` spelling is covered below by the no-error test, where an
     * unresolved container and a resolved one both have to stay quiet.
     */
    private val animals = """
        class Animal {
            public String nm;
            public Animal(String nm) { this.nm = nm; }
            public String describe() { return "animal " + this.nm; }
        }

        class Dog extends Animal {
            public Dog(String nm) { super(nm); }
            @Override public String describe() { return "dog " + this.nm; }
        }

        class Pen<T> {
            public T[] items;
            public Pen(T[] items) { this.items = items; }
        }
    """.trimIndent()

    /** The type of the binding named [name] in the open file, as source text. */
    private fun typeOfBinder(name: String): String? =
        PsiTreeUtil.collectElementsOfType(myFixture.file, JuxLocalVariable::class.java)
            .firstOrNull { it.name == name }
            ?.let { JuxTypeEngine.declaredType(it).presentable() }

    private fun parses(name: String, text: String) {
        myFixture.configureByText("$name.jux", text)
        val errors = PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java)
        assertTrue(
            "$name: " + errors.joinToString { "${it.errorDescription} @${it.textOffset}" },
            errors.isEmpty(),
        )
    }

    // ---------------------------------------------- E101: subclass patterns

    fun testSubclassPatternFormsParse() {
        // Every form §A.3 lists, over a sealed hierarchy rather than a record:
        // a literal part, a `_` part, `var` binders, alternatives that bind the
        // same names, and a guard that reads what the arm bound.
        parses(
            "sealedPatterns",
            """
            $lights

            String describe(Light l) {
                return switch (l) {
                    case Red(0, _)            -> "red, no wait";
                    case Red(var s, "urgent")  -> "urgent red";
                    case Red(var s, var n)     -> "red";
                    case Yellow(var s, _)      -> "yellow";
                    case Green(var s, true)    -> "green + arrow";
                    case Green(var s, _)       -> "green";
                };
            }

            int seconds(Light l) {
                return switch (l) {
                    case Red(var s, _) | Yellow(var s, _) -> s;
                    case Green(var s, _) -> s * 10;
                };
            }

            String big(Light l) {
                return switch (l) {
                    case Red(var s, _) when s > 20 -> "long red";
                    case Red r -> "short red";
                    default -> "not red";
                };
            }
            """.trimIndent(),
        )
    }

    fun testASubclassPatternBindsInstanceFieldsByPosition() {
        // E101: part `i` is the named subclass's `i`-th INSTANCE field in
        // declaration order. Only `recordComponents` answered before, which a
        // class has none of, so both binders came out unknown and the guard in
        // `case Red(var s, _) when s > 20` had nothing to type against.
        myFixture.configureByText(
            "bind.jux",
            """
            $lights

            String describe(Light l) {
                return switch (l) {
                    case Red(var secs, var why) -> why;
                    default -> "other";
                };
            }
            """.trimIndent(),
        )
        assertEquals("part 0 is Red's first instance field", "int", typeOfBinder("secs"))
        assertEquals("part 1 is Red's second instance field", "String", typeOfBinder("why"))
    }

    fun testAStaticFieldTakesNoPosition() {
        // §A.3: static fields are not instance state, so they take no position.
        // Counting `Yellow.LIMIT` would shift every binder after it and type
        // `flash` as an `int`.
        myFixture.configureByText(
            "statics.jux",
            """
            $lights

            String describe(Light l) {
                return switch (l) {
                    case Yellow(var secs, var flash) -> "y";
                    default -> "other";
                };
            }
            """.trimIndent(),
        )
        assertEquals("the static field is skipped, not counted", "int", typeOfBinder("secs"))
        assertEquals("so part 1 is still the second INSTANCE field", "bool", typeOfBinder("flash"))
    }

    fun testABarePartNameIsABinder() {
        // §A.3 admits a lone `identifier` as a binding "in tuple/record only",
        // so `case Circle(r)` is `case Circle(var r)` written shorter. Without a
        // LOCAL_VARIABLE node for it the name resolved to nothing: no rename, no
        // find-usages, and the unused-local inspection saw a use of an undeclared
        // name inside the arm.
        myFixture.configureByText(
            "bare.jux",
            """
            $lights

            String describe(Light l) {
                return switch (l) {
                    case Red(secs, why) -> why;
                    default -> "other";
                };
            }
            """.trimIndent(),
        )
        assertEquals("int", typeOfBinder("secs"))
        assertEquals("String", typeOfBinder("why"))
    }

    fun testAnUpperCasePartIsNotSwallowedAsABinder() {
        // The gate on the rule above. An upper-case part is an enum constant
        // used as a TEST, and a binder there would shadow the constant and match
        // every value instead of the one arm's.
        myFixture.configureByText(
            "enumpart.jux",
            """
            enum Colour { RED, GREEN }

            record Flag(Colour hue, int count) {}

            String f(Flag g) {
                return switch (g) {
                    case Flag(RED, var n) -> "red";
                    default -> "other";
                };
            }
            """.trimIndent(),
        )
        assertNull("`RED` stays a name, not a binding", typeOfBinder("RED"))
        assertEquals("int", typeOfBinder("n"))
    }

    fun testARecordPatternStillBindsComponents() {
        // The same entry point answers all three §A.3 meanings of `Name(...)`,
        // so the record case has to keep working.
        myFixture.configureByText(
            "record.jux",
            """
            record Point(int x, String label) {}

            String where(Point p) {
                return switch (p) {
                    case Point(var px, var tag) -> tag;
                };
            }
            """.trimIndent(),
        )
        assertEquals("int", typeOfBinder("px"))
        assertEquals("String", typeOfBinder("tag"))
    }

    // -------------------------------------- E100: members through a wildcard

    fun testAProducerWildcardIsReadAsItsBound() {
        // §T.4.8 rule 2: member resolution on a `? extends B` value runs against
        // `B`, because the lift bounds the synthetic parameter by `B`'s marker
        // trait and that marker carries `B`'s member surface. The argument used
        // to be DROPPED from the type argument list, so `Vec<? extends Animal>`
        // measured as a `Vec` of nothing and the binder came out unknown.
        myFixture.configureByText(
            "producer.jux",
            """
            $animals

            public void reportAnimals(Pen<? extends Animal> xs) {
                for (var a : xs) {
                    print(a.nm);
                    print(a.describe());
                }
            }
            """.trimIndent(),
        )
        assertEquals("a producer is read as its bound", "Animal", typeOfBinder("a"))
    }

    fun testAConsumerWildcardIsNotPeeled() {
        // The deliberate other half of §T.4.8 rule 2: PECS says a `? super B`
        // may be written and not read, so there is no member surface to resolve
        // against and peeling it would invent one. Unknown is what keeps the
        // editor silent instead of confident.
        myFixture.configureByText(
            "consumer.jux",
            """
            $animals

            public void fill(Pen<? super Dog> v) {
                for (var x : v) {
                    print(x);
                }
            }
            """.trimIndent(),
        )
        assertEquals("a consumer stays unpeeled", "?", typeOfBinder("x"))
    }

    fun testMembersThroughAWildcardAreNotFlaggedAndResolve() {
        // The editor-visible outcome, as the corpus test measures it: no error
        // highlight anywhere in the shape, and the field read resolves to the
        // bound's declaration rather than to nothing.
        myFixture.enableInspections(
            dev.jux.intellij.inspections.JuxUnresolvedReferenceInspection(),
            dev.jux.intellij.inspections.JuxAbstractNotImplementedInspection(),
        )
        myFixture.configureByText(
            "members.jux",
            """
            $animals

            public int totalNameLength(Vec<? extends Animal> xs) {
                int n = 0;
                for (var a : xs) {
                    n += a.describe().length();
                }
                return n;
            }
            """.trimIndent(),
        )
        val errors = myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
        assertTrue("unexpected errors: $errors", errors.isEmpty())
    }

    fun testAFieldReadThroughAWildcardResolvesToTheBoundsDeclaration() {
        // The chain end to end: the binder's type, the bound's member surface,
        // and the reference layer that reads both. Go-to and rename on `nm`
        // used to have nothing to land on, because the receiver had no type.
        myFixture.configureByText(
            "read.jux",
            """
            $animals

            public void show(Pen<? extends Animal> xs) {
                for (var a : xs) {
                    print(a.<caret>nm);
                }
            }
            """.trimIndent(),
        )
        val target = myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve()
        assertNotNull("`a.nm` resolves through the bound", target)
        val owner = PsiTreeUtil.getParentOfType(target, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)
        assertEquals("and it is Animal's field", "Animal", owner?.name)
    }
}
