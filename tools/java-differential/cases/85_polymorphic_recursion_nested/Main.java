import java.util.HashSet;
import java.util.Objects;

public class Main {
    static class Pair<T> {
        T left;
        T right;
        Pair(T left, T right) {
            this.left = left;
            this.right = right;
        }
        @Override
        public String toString() {
            return "(" + left + ", " + right + ")";
        }
        @Override
        public boolean equals(Object o) {
            if (!(o instanceof Pair)) {
                return false;
            }
            Pair<?> other = (Pair<?>) o;
            return Objects.equals(left, other.left) && Objects.equals(right, other.right);
        }
        @Override
        public int hashCode() {
            return Objects.hashCode(left) ^ Objects.hashCode(right);
        }
    }

    static abstract class Nested<T> {
        abstract int depth();
        abstract int size();
        abstract String describe();
    }

    static class Flat<T> extends Nested<T> {
        T value;
        Flat(T value) { this.value = value; }
        int depth() { return 0; }
        int size() { return 1; }
        String describe() { return "Flat " + value; }
    }

    static class Nest<T> extends Nested<T> {
        Nested<Pair<T>> inner;
        Nest(Nested<Pair<T>> inner) { this.inner = inner; }
        int depth() { return 1 + inner.depth(); }
        int size() { return 2 * inner.size(); }
        String describe() { return "Nest(" + inner.describe() + ")"; }
    }

    static <T> Nested<T> build(T x, int n) {
        if (n == 0) {
            return new Flat<T>(x);
        }
        return new Nest<T>(Main.<Pair<T>>build(new Pair<T>(x, x), n - 1));
    }

    static <T> boolean sameAfter(T a, T b, int n) {
        if (n == 0) {
            return a.equals(b);
        }
        return Main.<Pair<T>>sameAfter(new Pair<T>(a, a), new Pair<T>(b, b), n - 1);
    }

    public static void main(String[] args) {
        for (int d = 0; d <= 3; d++) {
            Nested<Integer> t = Main.<Integer>build(d + 1, d);
            System.out.println("depth " + t.depth() + ", size " + t.size() + ": " + t.describe());
        }
        Nested<String> s = Main.<String>build("s", 2);
        System.out.println(s.describe());

        System.out.println(Main.<Integer>sameAfter(3, 3, 4));
        System.out.println(Main.<String>sameAfter("a", "b", 4));

        HashSet<Pair<Integer>> pairs = new HashSet<Pair<Integer>>();
        pairs.add(new Pair<Integer>(1, 2));
        pairs.add(new Pair<Integer>(1, 2));
        pairs.add(new Pair<Integer>(2, 1));
        System.out.println(pairs.size());

        Flat<Integer> one = new Flat<Integer>(1);
        HashSet<Flat<Integer>> seen = new HashSet<Flat<Integer>>();
        seen.add(one);
        seen.add(one);
        seen.add(new Flat<Integer>(1));
        System.out.println(seen.size());
        System.out.println(one.value + 41);
    }
}
