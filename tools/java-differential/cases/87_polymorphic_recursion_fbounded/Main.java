public class Main {
    interface Ord2<T> {
        int cmp(T other);
        T pick(T other);
    }

    static class Num implements Ord2<Num> {
        int v;
        Num(int v) { this.v = v; }
        public int cmp(Num other) { return v - other.v; }
        public Num pick(Num other) { return cmp(other) >= 0 ? this : other; }
        @Override public String toString() { return "" + v; }
    }

    static class Box<T extends Ord2<T>> implements Ord2<Box<T>> {
        T x;
        Box(T x) { this.x = x; }
        public int cmp(Box<T> other) { return x.cmp(other.x); }
        public Box<T> pick(Box<T> other) { return cmp(other) >= 0 ? this : other; }
        @Override public String toString() { return "<" + x + ">"; }
    }

    static <T extends Ord2<T>> String best(T a, T b, int n) {
        T p = a.pick(b);
        String here = n + ": " + p + " cmp " + a.cmp(b) + " again " + p.cmp(a);
        if (n == 0) { return here; }
        return here + " | " + Main.<Box<T>>best(new Box<T>(a), new Box<T>(b), n - 1);
    }

    interface Tagged<T, L> {
        String tagged(T other, L label);
    }

    static class Leaf implements Tagged<Leaf, String> {
        String name;
        Leaf(String name) { this.name = name; }
        public String tagged(Leaf other, String label) { return label + ":" + name + "/" + other.name; }
    }

    static class Wrap<T extends Tagged<T, String>> implements Tagged<Wrap<T>, String> {
        T inner;
        Wrap(T inner) { this.inner = inner; }
        public String tagged(Wrap<T> other, String label) { return "w(" + inner.tagged(other.inner, label) + ")"; }
    }

    static <T extends Tagged<T, String>> String deep(T a, T b, int n) {
        if (n == 0) { return a.tagged(b, "at0"); }
        return Main.<Wrap<T>>deep(new Wrap<T>(a), new Wrap<T>(b), n - 1);
    }

    static abstract class Shape<T extends Shape<T>> {
        int sides;
        Shape(int sides) { this.sides = sides; }
        abstract T grown();
        abstract String label();
        boolean bigger(T other) { return label().length() > other.label().length(); }
    }

    static class Poly extends Shape<Poly> {
        Poly(int sides) { super(sides); }
        Poly grown() { return new Poly(sides + 1); }
        String label() { return "poly" + sides; }
    }

    static class Framed<T extends Shape<T>> extends Shape<Framed<T>> {
        T inner;
        Framed(T inner) { super(0); this.inner = inner; }
        Framed<T> grown() { return new Framed<T>(inner.grown()); }
        String label() { return "[" + inner.label() + "]"; }
    }

    static <T extends Shape<T>> String climb(T s, int n) {
        T g = s.grown();
        String here = s.label() + "->" + g.label() + " " + g.bigger(s);
        if (n == 0) { return here; }
        return here + " | " + Main.<Framed<T>>climb(new Framed<T>(s), n - 1);
    }

    static class Pair<T> {
        T a;
        T b;
        Pair(T a, T b) { this.a = a; this.b = b; }
    }

    interface Joins<P> {
        String join(P pair);
    }

    static class Word implements Joins<Pair<Word>> {
        String s;
        Word(String s) { this.s = s; }
        public String join(Pair<Word> pair) { return pair.a.s + "+" + pair.b.s + "@" + s; }
    }

    static class Shell<T extends Joins<Pair<T>>> implements Joins<Pair<Shell<T>>> {
        T inner;
        Shell(T inner) { this.inner = inner; }
        public String join(Pair<Shell<T>> pair) {
            return "(" + inner.join(new Pair<T>(pair.a.inner, pair.b.inner)) + ")";
        }
    }

    static <T extends Joins<Pair<T>>> String weave(T x, T y, int n) {
        if (n == 0) { return x.join(new Pair<T>(x, y)); }
        return Main.<Shell<T>>weave(new Shell<T>(x), new Shell<T>(y), n - 1);
    }

    public static void main(String[] args) {
        System.out.println(Main.<Num>best(new Num(3), new Num(8), 3));
        System.out.println(Main.<Leaf>deep(new Leaf("a"), new Leaf("b"), 3));
        System.out.println(Main.<Poly>climb(new Poly(9), 3));
        System.out.println(Main.<Word>weave(new Word("a"), new Word("b"), 3));
    }
}
