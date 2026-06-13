package dev.jux.intellij.parser

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.ParsingTestCase
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxEnumConstant
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParserDefinition
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxReference
import java.io.File

/**
 * Parses every `.jux` file in the repository's `examples/` corpus through the
 * real [JuxParser] and asserts the resulting PSI tree has no [PsiErrorElement]s.
 *
 * This is the lenient-superset acceptance bar from the implementation plan: the
 * declaration-level parser must accept the whole corpus without red squiggles
 * (method bodies are opaque, so the bar covers the compilation unit, type
 * declarations, members, and signatures).
 */
class JuxParsingTest : ParsingTestCase("", "jux", JuxParserDefinition()) {

    fun testAllExamplesParseWithoutErrors() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)

        val failures = StringBuilder()
        var count = 0
        examples.listFiles { _, name -> name.endsWith(".jux") }!!.sortedBy { it.name }.forEach { file ->
            count++
            val psi = createPsiFile(file.name, file.readText())
            val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
            if (errors.isNotEmpty()) {
                failures.appendLine("• ${file.name}:")
                errors.take(5).forEach { failures.appendLine("    ${it.errorDescription} @ ${it.textOffset}") }
            }
        }
        assertTrue("parsed $count files", count > 0)
        assertTrue("parse errors found:\n$failures", failures.isEmpty())
    }

    /** Validates the named-declaration PSI that Structure View / navigation use. */
    fun testPsiNamesAndMembers() {
        val psi = createPsiFile(
            "Sample.jux",
            """
            package demo;
            public class Greeter {
                private int count;
                public void greet(String who) { print(who); }
            }
            public enum Color { Red, Green, Blue }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java))

        val types = PsiTreeUtil.collectElementsOfType(psi, JuxTypeDeclaration::class.java).map { it.name }.toSet()
        assertEquals(setOf("Greeter", "Color"), types)

        val methods = PsiTreeUtil.collectElementsOfType(psi, JuxMethodDeclaration::class.java).map { it.name }
        assertTrue("greet method", methods.contains("greet"))

        val fields = PsiTreeUtil.collectElementsOfType(psi, JuxFieldDeclaration::class.java).map { it.name }
        assertTrue("count field", fields.contains("count"))

        val constants = PsiTreeUtil.collectElementsOfType(psi, JuxEnumConstant::class.java).map { it.name }.toSet()
        assertEquals(setOf("Red", "Green", "Blue"), constants)
    }

    /**
     * The PSI structure the override-methods Generate action relies on: a
     * class's `implements`/`extends` clause exposes its supertype names as
     * TYPE_REFERENCE nodes, and the class body holds its methods as
     * METHOD_DECLARATION children — so the action can read supertype names and
     * method signatures off the tree.
     */
    fun testOverrideActionPsiShape() {
        val psi = createPsiFile(
            "Shape.jux",
            """
            package demo;
            public interface Shape { double area(); String name(); }
            public class Circle implements Shape {
                public double area() { return 3.14; }
            }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java))

        val circle = PsiTreeUtil.collectElementsOfType(psi, JuxTypeDeclaration::class.java)
            .first { it.name == "Circle" }
        // implements clause carries the supertype reference `Shape`.
        val impl = circle.node.findChildByType(dev.jux.intellij.psi.JuxElementTypes.IMPLEMENTS_CLAUSE)
        assertNotNull("implements clause present", impl)
        val refs = PsiTreeUtil.collectElements(impl!!.psi) {
            it.node.elementType == dev.jux.intellij.psi.JuxElementTypes.TYPE_REFERENCE
        }.map { it.text.trim() }
        assertTrue("supertype Shape referenced", refs.any { it.contains("Shape") })

        // The class body exposes its own method as a METHOD_DECLARATION child.
        val body = circle.node.findChildByType(dev.jux.intellij.psi.JuxElementTypes.CLASS_BODY)
        assertNotNull("class body present", body)
        val ownMethods = body!!.psi.children
            .filter { it.node.elementType == dev.jux.intellij.psi.JuxElementTypes.METHOD_DECLARATION }
            .mapNotNull { (it as? dev.jux.intellij.psi.JuxNamedElement)?.name }
        assertTrue("Circle declares area()", ownMethods.contains("area"))
    }

    /**
     * Constructs that the PSI parser must accept without red squiggles, pinned
     * explicitly (the corpus test covers them via examples/, but this guards the
     * exact shapes against future parser edits):
     *  - `init { }` instance-initializer blocks,
     *  - pointer types `int*` and a deref *statement* `*p = value;` (must parse
     *    as an expression statement, not a bogus `type=*, name=p` local),
     *  - the `=>` type-test with a smart-cast binder, both `as` and C-style
     *    downcasts, `super.method()`, and nullable `T?` value flow.
     */
    fun testNewLanguageConstructsParseWithoutErrors() {
        val psi = createPsiFile(
            "New.jux",
            """
            public abstract class Animal { public abstract String sound(); }
            public class Dog extends Animal {
                public int hits;
                public Dog() {}
                public String sound() { return super.toString(); }
                // instance-initializer block
                init { this.hits = 0; }
            }
            public unsafe void store(int* p, int value) { *p = value; }
            public void main() {
                Animal? a = new Dog();
                if (a != null) {
                    if (a => Dog d) { print(d.sound()); }
                    Dog viaC = (Dog) a;
                    Dog viaAs = a as Dog;
                    print(viaC.sound());
                    print(viaAs.sound());
                }
            }
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription}@${it.textOffset}" },
            errors.isEmpty(),
        )
    }

    /**
     * Generic clauses decompose into navigable PSI: type-parameter names, bound
     * type references, and wildcards become real nodes (so find-usages /
     * go-to-definition / rename reach type names inside `<…>`). Deeply nested
     * generics — whose closing run lexes as merged `>>` tokens — must still
     * balance without a stray error.
     */
    fun testGenericsDecomposeIntoReferences() {
        val psi = createPsiFile(
            "Generics.jux",
            """
            public abstract class Animal {}
            public interface Speaks {}
            public class Pair<A, B> { public A first; public B second; public Pair() {} }
            public class Holder<T extends Animal & Speaks> { public T value; public Holder() {} }
            public class Deep { public Pair<Pair<Pair<Pair<String>>>> nested; public Deep() {} }
            public void describe(Pair<? extends Animal, ? super Speaks> p) {}
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription}@${it.textOffset}" },
            errors.isEmpty(),
        )

        // Declared type-parameter names are TYPE_PARAMETER nodes (`A`, `B`, `T`).
        val typeParams = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.TYPE_PARAMETER
        }.map { it.text.trim() }.toSet()
        assertTrue("type params A,B,T decomposed (got $typeParams)", typeParams.containsAll(setOf("A", "B", "T")))

        // Wildcards become WILDCARD_TYPE nodes (`? extends Animal`, `? super Speaks`).
        val wildcards = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.WILDCARD_TYPE
        }
        assertEquals("two wildcards decomposed", 2, wildcards.size)

        // The bound `Animal` is a TYPE_REFERENCE inside a wildcard — navigable.
        val refTexts = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.TYPE_REFERENCE
        }.map { it.text.trim() }
        assertTrue("bound type reference Animal present (got $refTexts)", refTexts.any { it == "Animal" })
    }

    /**
     * Recent-language-surface constructs the plugin parser must produce the
     * right PSI for (not merely accept):
     *  - `static { }` initializer blocks become STATIC_BLOCK (distinct from
     *    instance INIT_BLOCK), per §S.4.1;
     *  - const generics `<int N>` (§A.2.6) — the parameter NAME `N` is the
     *    TYPE_PARAMETER node, with `int` decomposed as a type reference;
     *  - the wrapping operators `+%` / `-%` / `*%` / `<<%` / `>>%` lex as
     *    single atoms and parse as ordinary binary expressions.
     */
    fun testStaticBlockConstGenericsAndWrappingOps() {
        val psi = createPsiFile(
            "Recent.jux",
            """
            public class Buffer<int N> {
                public int used;
                static { print("loaded"); }
                init { this.used = 0; }
                public Buffer() {}
                public int grow(int a, int b) {
                    var sum = a +% b;
                    var diff = a -% b;
                    var prod = a *% b;
                    var shl = a <<% 1;
                    var shr = a >>% 1;
                    return sum +% diff *% prod + shl - shr;
                }
            }
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription}@${it.textOffset}" },
            errors.isEmpty(),
        )

        // `static { }` is a STATIC_BLOCK; `init { }` stays an INIT_BLOCK.
        val staticBlocks = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.STATIC_BLOCK
        }
        assertEquals("one static block", 1, staticBlocks.size)
        val initBlocks = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.INIT_BLOCK
        }
        assertEquals("one init block", 1, initBlocks.size)

        // The const-generic's declared name is `N`, not the value type `int`.
        val typeParams = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.TYPE_PARAMETER
        }.map { it.text.trim() }
        assertEquals("const-generic parameter name", listOf("N"), typeParams)
    }

    /**
     * Pass-2 bug-hunt regressions:
     *  - expression-bodied properties use `->` (§M.7.4) — the parser used to
     *    accept only `=>` and painted red errors on spec-legal code;
     *  - `sizeof` can follow `?` in a ternary (it's an expression starter, so
     *    `flag ? sizeof(int) : 4` must not parse `flag?` as error-propagation)
     *    and can follow a C-style cast;
     *  - `yield expr;` parses as a lenient statement (reserved §M.2 keyword,
     *    must not red-squiggle).
     */
    fun testPropertyArrowSizeofTernaryAndYieldParse() {
        val psi = createPsiFile(
            "Pass2.jux",
            """
            public class Config {
                private int w;
                public Config() {}
                public String name -> "ident";
                public int doubled -> this.w * 2;
                public void gen() {
                    yield 42;
                }
                public int pick(bool flag) {
                    var r = flag ? sizeof(int) : 4;
                    return (int) sizeof(long) + r;
                }
            }
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription}@${it.textOffset}" },
            errors.isEmpty(),
        )
        // The `-> expr` shorthands are PROPERTY_DECLARATIONs, still visible to
        // field consumers via the JuxFieldDeclaration base class.
        val fields = PsiTreeUtil.collectElementsOfType(psi, JuxFieldDeclaration::class.java).map { it.name }
        assertTrue("property `name` parsed as a member (got $fields)", fields.contains("name"))
        assertTrue("property `doubled` parsed as a member (got $fields)", fields.contains("doubled"))
    }

    /**
     * Observable properties (§P): every accessor-block shape from the probes
     * corpus parses error-free into PROPERTY_DECLARATION with structured
     * PROPERTY_ACCESSOR children, while plain fields stay FIELD_DECLARATION.
     */
    fun testObservablePropertyShapesParse() {
        val psi = createPsiFile(
            "Props.jux",
            """
            package demo;
            public class Model {
                public int Size { get; set; } = 0;
                public String Name { get; set; } = "";
                public bool Visible { get; set; };
                public String Now { get; set; }
                public String Id { get; private set; } = "";
                public int Count { get; protected set; } = 0;
                private String test { get; set; } = "test";
                private int plainField;
                private int _age;
                public int Age {
                    get -> _age;
                    set { if (value > 0) { _age = value; } }
                };
                public double Celsius { get; set; } = 0.0;
                public double Fahrenheit {
                    get -> Celsius * 9.0 / 5.0 + 32.0;
                    set { Celsius = (value - 32.0) * 5.0 / 9.0; }
                };
                public bool IsEmpty { get -> Size == 0; };
                public String Label -> "x";
                private final observer<String> nameObs = (old, now) -> {
                    print(now);
                };
                public Model() {}
                public void use(Model m) {
                    m.Name.observers.attach(this.nameObs);
                    m.Name.observers.attach((old, now) -> { print(now); });
                    m.Name.observers.detach(this.nameObs);
                    m.Name.observers.clear;
                    print(m.Name.observers.size);
                    m.Name.bind(m.Id);
                    m.Name.unbind();
                    m.Size.bindBidirectional(m.Count);
                    m.Name = "x";
                }
            }
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription}@${it.textOffset}" },
            errors.isEmpty(),
        )

        // Accessor-block and `-> expr` members are PROPERTY_DECLARATIONs …
        val props = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.PROPERTY_DECLARATION
        }.map { (it as JuxNamedElement).name }.toSet()
        assertEquals(
            setOf(
                "Size", "Name", "Visible", "Now", "Id", "Count", "test",
                "Age", "Celsius", "Fahrenheit", "IsEmpty", "Label",
            ),
            props,
        )
        // … while plain fields (incl. the observer variable) stay FIELD_DECLARATION.
        val fields = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.FIELD_DECLARATION
        }.map { (it as JuxNamedElement).name }.toSet()
        assertEquals(setOf("plainField", "_age", "nameObs"), fields)

        // Accessor structure: `Id` has two accessors, the setter carrying a
        // `private` modifier list; `IsEmpty` has a single expression-form getter.
        val id = PsiTreeUtil.collectElements(psi) {
            it.elementType === JuxElementTypes.PROPERTY_DECLARATION && (it as JuxNamedElement).name == "Id"
        }.single()
        val idAccessors = PsiTreeUtil.collectElements(id) {
            it.elementType === JuxElementTypes.PROPERTY_ACCESSOR
        }
        assertEquals("Id has get + set accessors", 2, idAccessors.size)
        assertTrue(
            "Id setter carries private modifier",
            idAccessors.any { acc ->
                acc.text.contains("private") &&
                    acc.node.findChildByType(JuxElementTypes.MODIFIER_LIST) != null
            },
        )
    }

    /** The [JuxPropertyDeclaration] PSI helpers the §P annotator/inspections rely on. */
    fun testPropertyPsiHelpers() {
        val psi = createPsiFile(
            "PropPsi.jux",
            """
            public class C {
                public String Id { get; private set; } = "";
                public int Age {
                    get -> 1;
                    set { if (value > 0) { print(value); } }
                };
                public bool IsEmpty { get -> true; };
                public String Label -> "x";
                private int Hidden { get; set; };
            }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java))
        val props = PsiTreeUtil.collectElementsOfType(psi, JuxPropertyDeclaration::class.java)
            .associateBy { it.name }
        assertEquals(setOf("Id", "Age", "IsEmpty", "Label", "Hidden"), props.keys)

        val id = props.getValue("Id")
        assertTrue("Id has a setter", id.hasSetter())
        assertFalse("Id is not computed", id.isComputed())
        assertNull("Id getter inherits visibility", id.accessorVisibility(id.getterAccessor()!!))
        assertEquals(
            "Id setter is private",
            JuxTokenTypes.PRIVATE_KW,
            id.accessorVisibility(id.setterAccessor()!!),
        )
        assertNull("Id auto-setter has no block body", id.setterBody())
        assertTrue("Id is public", id.isPublic())
        assertEquals("String", id.typeText())

        val age = props.getValue("Age")
        assertNotNull("Age setter has a block body", age.setterBody())

        val isEmpty = props.getValue("IsEmpty")
        assertTrue("get-only block is computed", isEmpty.isComputed())

        val label = props.getValue("Label")
        assertTrue("-> shorthand is computed", label.isComputed())
        assertNull("-> shorthand has no accessor list", label.accessorList())

        val hidden = props.getValue("Hidden")
        assertTrue("Hidden is private", hidden.isPrivate())
        assertFalse("Hidden is not public", hidden.isPublic())

        // Plain fields never come back as JuxPropertyDeclaration.
        val plain = createPsiFile("Plain.jux", "public class D { private int count; }")
        assertEmpty(PsiTreeUtil.collectElementsOfType(plain, JuxPropertyDeclaration::class.java))
    }

    /** The removed `init` accessor (§P) yields a targeted error, nothing more. */
    fun testInitAccessorDiagnosed() {
        val psi = createPsiFile(
            "InitProp.jux",
            """
            public class C {
                public String Id { get; init; }
            }
            """.trimIndent(),
        )
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertEquals("exactly one error for the removed init accessor", 1, errors.size)
        assertTrue(
            "error mentions the init removal (got '${errors.first().errorDescription}')",
            errors.first().errorDescription.contains("init"),
        )
    }

    /**
     * Testing-framework shapes (sec. TS.1): an annotated free function parses
     * error-free into a top-level METHOD_DECLARATION whose ANNOTATION nodes
     * are its direct leading children — the structure JuxTestDetector and the
     * placement inspection rely on. `for await` (sec. 18.6) parses too.
     */
    fun testAnnotatedFreeFunctionsAndForAwaitParse() {
        val psi = createPsiFile(
            "Tests.jux",
            """
            package demo;
            import jux.std.testing.*;

            @BeforeEach
            void setup() {}

            @Test
            async void streams() {
                for await (var x : source) {
                    assertTrue(x > 0);
                }
            }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java).toList())

        val fns = psi.children.filterIsInstance<JuxMethodDeclaration>()
        assertEquals(listOf("setup", "streams"), fns.map { it.name })
        for (fn in fns) {
            val ann = fn.children.firstOrNull { it.elementType === JuxElementTypes.ANNOTATION }
            assertNotNull("annotation is a direct child of ${fn.name}", ann)
        }
        assertTrue(fns[0].children.any { it.elementType === JuxElementTypes.ANNOTATION && it.text == "@BeforeEach" })
        assertTrue(fns[1].children.any { it.elementType === JuxElementTypes.ANNOTATION && it.text == "@Test" })
    }

    /**
     * `ref` reference declarations (`public ref String x`) — pre-wired like
     * `typeof`; the assertions only run once the compiler reserves the keyword
     * and `jux-tokens.json` regenerates, so this passes before AND after.
     */
    fun testRefDeclarationsParseOnceReserved() {
        if ("ref" !in dev.jux.intellij.highlight.JuxKeywords.KEYWORDS) return
        val psi = createPsiFile(
            "Refs.jux",
            """
            public class Holder {
                public ref String shared;
                public void use(ref String s) {
                    ref String alias = shared;
                    for (ref String x : items) {
                        print(x);
                    }
                }
            }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java).toList())
    }

    /** A use of a type name resolves to its in-file declaration. */
    fun testReferenceResolvesToDeclaration() {
        val file = createPsiFile(
            "Ref.jux",
            """
            public class Box { public int size() { return 0; } }
            public class User { public void run() { var b = new Box(); } }
            """.trimIndent(),
        )
        // The `Box` identifier inside `new Box()` lives under a TYPE_REFERENCE.
        val use = file.firstChild.let { collectIdentifiers(file) }
            .first { it.text == "Box" && it.parent.elementType === JuxElementTypes.TYPE_REFERENCE }
        val resolved = JuxReference(use, TextRange(0, use.textLength)).resolve()
        assertTrue("resolves to the Box class", resolved is JuxTypeDeclaration && (resolved as JuxNamedElement).name == "Box")
    }

    private fun collectIdentifiers(root: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        PsiTreeUtil.processElements(root) { e ->
            if (e.elementType === JuxTokenTypes.IDENTIFIER) out.add(e)
            true
        }
        return out
    }

    // Examples live at <repo>/examples; the plugin module is <repo>/ide/intellij-plugin.
    override fun getTestDataPath(): String = File("../../examples").absolutePath

    override fun skipSpaces(): Boolean = false
    override fun includeRanges(): Boolean = true
}
