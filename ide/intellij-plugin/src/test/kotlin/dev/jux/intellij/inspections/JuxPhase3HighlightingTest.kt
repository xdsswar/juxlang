package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Whole programs in the syntax the compiler's Phase 3 adds raise no error and
 * no warning with every inspection on: the editor must not flag code that
 * builds (switch patterns, compact record constructors, enum fields,
 * interface `super` calls, free operators, `?: throw`, destructuring, labeled
 * blocks, the `assert` statement).
 */
class JuxPhase3HighlightingTest : BasePlatformTestCase() {
    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxAccessorVisibilityInspection(),
            JuxBindTypeMismatchInspection(),
            JuxBoundPropertyAssignmentInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxInheritedTypeParamInspection(),
            JuxMisplacedAccessorBlockInspection(),
            JuxMissingOverrideInspection(),
            JuxPropertyNamingInspection(),
            JuxPropertyNeverObservedInspection(),
            JuxRedundantSemicolonInspection(),
            JuxSetterEarlyReturnInspection(),
            JuxTestAnnotationPlacementInspection(),
            JuxUnreachableCodeInspection(),
            JuxUnresolvedReferenceInspection(),
            JuxUnusedImportInspection(),
            JuxUnusedLocalSymbolInspection(),
            JuxEmptyCatchBlockInspection(),
            JuxRedundantCastInspection(),
            JuxConstantConditionInspection(),
            JuxRedundantElseInspection(),
            JuxPointlessBooleanExpressionInspection(),
            JuxUnusedPrivateMemberInspection(),
            JuxDuplicateConditionInspection(),
            JuxSelfAssignmentInspection(),
            JuxEmptyStatementBodyInspection(),
            JuxMissingReturnInspection(),
            JuxAbstractMethodInClassInspection(),
            JuxUnhandledExceptionInspection(),
            JuxPackageMismatchInspection(),
        )
    }

    private fun probe(name: String, text: String) {
        myFixture.configureByText("$name.jux", text.replace("\$", "$"))
        val hl = myFixture.doHighlighting()
            .filter { it.severity == HighlightSeverity.ERROR || it.severity == HighlightSeverity.WARNING }
            .map { "${it.severity} ${it.description} [${it.text}]" }
            .distinct()
        assertTrue("$name: " + hl.joinToString(" || "), hl.isEmpty())
    }

    fun testPhase3ProgramsHighlightClean() {
        probe("patterns", """
enum Day { Mon, Sat, Sun }
sealed interface Shape {}
record Circle(double r) implements Shape {}
record Sq(double s) implements Shape {}
String kind(Day d) { return switch (d) { case Sat, Sun -> "weekend"; default -> "weekday"; }; }
double area(Shape s) { return switch (s) { case Circle(var r) -> 3.0 * r * r; case Sq(var a) -> a * a; }; }
String grade(int n) { return switch (n) { case ..0 -> "neg"; case 1..10 -> "small"; case 11.. -> "big"; }; }
void main() { print(kind(Day.Sat)); print(area(new Sq(2.0))); print(grade(5)); }
""")
        probe("records", """
record Range(int lo, int hi) {
    Range {
        if (lo > hi) { throw new IllegalArgumentException("bad"); }
    }
    Range(int single) { this(single, single); }
}
enum Planet {
    Mercury(3.303e+23, 2.4397e6), Earth(5.976e+24, 6.37814e6);
    private final double mass;
    private final double radius;
    Planet(double mass, double radius) { this.mass = mass; this.radius = radius; }
    public double gravity() { return 6.67300E-11 * mass / (radius * radius); }
}
enum Color { Red, Green }
void main() {
    print(new Range(1, 2));
    print(new Range(5));
    print(Planet.Earth.gravity() > 9.0);
    print(Color.fromName("Green"));
    print(Color.fromOrdinal(0));
    print(Color.cases().length);
}
""")
        probe("ifaces", """
interface A { default String hi() { return "A"; } }
interface B { default String hi() { return "B"; } }
class C implements A, B { public String hi() { return A.super.hi() + B.super.hi(); } }
interface Sized { int Size { get; } default bool isEmpty -> Size == 0; }
class Bag implements Sized { public int Size { get; } = 0; }
record Vec3(double x, double y, double z) {}
Vec3 operator*(double k, Vec3 v) { return new Vec3(k * v.x, k * v.y, k * v.z); }
class Animal {}
class Dog extends Animal { public void bark() { print("woof"); } }
String need(String? s) { return s ?: throw new IllegalArgumentException("none"); }
class Node { public int v; public Node? next; public Node(int v) { this.v = v; } }
void main() {
    print(new C().hi());
    print(new Bag().isEmpty);
    print(2.0 * new Vec3(1.0, 2.0, 3.0));
    Animal a = new Dog();
    if (a => Dog) a.bark();
    print(need("x"));
    int? x = 3;
    assert x != null;
    print(x + 1);
    Node? cur = new Node(1);
    int sum = 0;
    while (cur != null) { sum += cur.v; cur = cur.next; }
    print(sum);
}
""")
        probe("decls", """
record Pt(int x, int y) {}
int twice(int x) = x * 2;
void main() {
    var p = new Pt(1, 2);
    var Pt(a, b) = p;
    (int, int) t = (1, 2);
    print(a + b + t.0 + t.1 + twice(2));
    blk: {
        if (a > 0) { break blk; }
        print("unreached");
    }
    ;
    assert a > 0 : "positive";
    print(typeof(5));
}
""")
    }
}
