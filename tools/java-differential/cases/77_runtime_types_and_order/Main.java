import java.util.function.BiFunction;
import java.util.function.DoubleFunction;

public class Main {
    static class Member {
        private static int next = 1;
        private String memberId;
        Member() {
            this.memberId = "M" + next;
            next++;
        }
        String id() { return memberId; }
    }

    interface Cmd { int id(); }
    static class Ins implements Cmd {
        int count;
        Ins(int count) { this.count = count; }
        public int id() { return 1; }
    }
    static class Stop implements Cmd { public int id() { return 2; } }
    static class Noop implements Cmd { public int id() { return 3; } }

    static String describe(Cmd c) {
        switch (c) {
            case Ins i when i.count > 10 -> { return "big insert " + i.count; }
            case Ins i -> { return "insert " + i.count; }
            case Stop s -> {
                String label = "stop " + s.id();
                return label;
            }
            default -> { return "other " + c.id(); }
        }
    }

    static abstract class Animal {}
    static class Dog extends Animal {}
    static class Cat extends Animal {}
    static String kind(Animal a) {
        return switch (a) {
            case Dog d -> "dog";
            case Cat c -> "cat";
            default -> "animal";
        };
    }

    interface Marker {}
    static class Tagged implements Marker { int tag = 7; }

    static class Pair<A, B> {
        A first;
        B second;
        Pair(A a, B b) { this.first = a; this.second = b; }
    }
    static <K, V> String showPair(Pair<K, V> p) { return "<" + p.first + ", " + p.second + ">"; }

    static class Holder {
        DoubleFunction<String> render = x -> "value " + x;
    }

    static class Cache<K, V> {
        private BiFunction<K, V, String> cb;
        Cache(BiFunction<K, V, String> cb) { this.cb = cb; }
        String fire(K k, V v) { return this.cb.apply(k, v); }
    }

    public static void main(String[] args) {
        System.out.println(new Member().id());
        System.out.println(new Member().id());

        System.out.println(describe(new Ins(3)));
        System.out.println(describe(new Ins(30)));
        System.out.println(describe(new Stop()));
        System.out.println(describe(new Noop()));
        System.out.println(kind(new Dog()));
        System.out.println(kind(new Cat()));

        Marker m = new Tagged();
        if (m instanceof Tagged t) { System.out.println("tagged " + t.tag); } else { System.out.println("not tagged"); }
        System.out.println(m instanceof Tagged);

        System.out.println(showPair(new Pair<String, Integer>("answer", 42)));

        System.out.println(new Holder().render.apply(5.0));

        String s = "ab";
        try {
            System.out.println(s.substring(1, 9));
        } catch (IndexOutOfBoundsException e) {
            System.out.println("caught " + e.getMessage());
        }
        try {
            System.out.println(s.substring(3));
        } catch (IndexOutOfBoundsException e) {
            System.out.println("caught " + e.getMessage());
        }
        try {
            System.out.println(s.charAt(5));
        } catch (IndexOutOfBoundsException e) {
            System.out.println("caught " + e.getMessage());
        }
        System.out.println(s.substring(1) + s.charAt(0));

        var cache = new Cache<String, Integer>((k, v) -> k + "=" + v);
        System.out.println(cache.fire("n", 4));
    }
}
