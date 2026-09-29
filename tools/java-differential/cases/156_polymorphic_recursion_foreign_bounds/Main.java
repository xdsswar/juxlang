// Java has no Rust containers: a tuple is a record, a Box and an Rc are
// small holder classes, `Rc.ptr_eq` is reference equality, and `T?` is a
// nullable reference. The program is otherwise the same.
public class Main {
    record Pair<A, B>(A a, B b) {}
    static final class Box<T> { final T v; Box(T v) { this.v = v; } }
    static final class Rc<T> { final T v; Rc(T v) { this.v = v; } }

    interface Pairs<P> { int score(P p); P make(); }
    interface Maybe<M> { int size(M m); M none(); }
    interface Shares<S> { boolean same(S a, S b); S keep(S s); }
    interface Boxes<B> { int weigh(B b); }

    static class Leaf implements Pairs<Pair<Leaf, Integer>>, Maybe<Leaf>, Shares<Rc<Leaf>>, Boxes<Box<Leaf>> {
        int id;
        Leaf(int id) { this.id = id; }
        public int score(Pair<Leaf, Integer> p) { return p.a().id * 10 + p.b(); }
        public Pair<Leaf, Integer> make() { return new Pair<>(this, 7); }
        public int size(Leaf m) { return m == null ? 0 : m.id; }
        public Leaf none() { return null; }
        public boolean same(Rc<Leaf> a, Rc<Leaf> b) { return a == b; }
        public Rc<Leaf> keep(Rc<Leaf> s) { return s; }
        public int weigh(Box<Leaf> b) { return 5; }
    }

    static class Wrap<T extends Pairs<Pair<T, Integer>> & Maybe<T> & Shares<Rc<T>> & Boxes<Box<T>>>
            implements Pairs<Pair<Wrap<T>, Integer>>, Maybe<Wrap<T>>, Shares<Rc<Wrap<T>>>, Boxes<Box<Wrap<T>>> {
        T inner;
        Wrap(T inner) { this.inner = inner; }
        public int score(Pair<Wrap<T>, Integer> p) { return 100 + p.b() + inner.score(inner.make()); }
        public Pair<Wrap<T>, Integer> make() { return new Pair<>(this, 8); }
        public int size(Wrap<T> m) { return m == null ? 0 : 1 + inner.size(inner); }
        public Wrap<T> none() { return null; }
        public boolean same(Rc<Wrap<T>> a, Rc<Wrap<T>> b) { return a == b; }
        public Rc<Wrap<T>> keep(Rc<Wrap<T>> s) { return s; }
        public int weigh(Box<Wrap<T>> b) { return 6; }
    }

    static <T extends Pairs<Pair<T, Integer>> & Maybe<T> & Shares<Rc<T>> & Boxes<Box<T>>> String run(T x, int n) {
        if (n > 0) { return run(new Wrap<T>(x), n - 1); }
        Pair<T, Integer> made = x.make();
        int s = x.score(made);
        int z = x.size(x.none()) * 100 + x.size(x);
        Rc<T> r = new Rc<>(x);
        Rc<T> back = x.keep(r);
        boolean same = x.same(r, back) && x.same(back, r);
        Rc<T> other = new Rc<>(x);
        boolean differs = x.same(r, other);
        int w = x.weigh(new Box<>(x));
        return s + " " + z + " " + same + " " + differs + " " + w;
    }

    public static void main(String[] args) {
        System.out.println(run(new Leaf(3), 0));
        System.out.println(run(new Leaf(3), 2));
    }
}
