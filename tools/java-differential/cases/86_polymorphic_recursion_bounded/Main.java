import java.util.ArrayList;

public class Main {
    interface Named {
        String name();
    }

    interface Aged {
        int age();
    }

    interface Sized2 extends Named {
        int size();
    }

    static class Tag implements Sized2, Aged {
        String label;
        Tag(String label) { this.label = label; }
        public String name() { return label; }
        public int size() { return 1; }
        public int age() { return 3; }
    }

    static class Wrap<T extends Sized2> implements Sized2 {
        T inner;
        Wrap(T inner) { this.inner = inner; }
        public String name() { return "wrap(" + inner.name() + ")"; }
        public int size() { return inner.size() + 1; }
    }

    static String describe(Named n) {
        return "named " + n.name();
    }

    static <T extends Sized2> String nest(T x, int n, ArrayList<String> log) {
        log.add(n + ": " + x.name() + " size " + x.size() + ", " + describe(x));
        if (n == 0) {
            return x.name();
        }
        return Main.<Wrap<T>>nest(new Wrap<T>(x), n - 1, log);
    }

    static abstract class Animal {
        abstract String sound();
        int legs() { return 4; }
    }

    static class Dog extends Animal {
        String sound() { return "woof"; }
    }

    static class Pen<A extends Animal> extends Animal {
        A held;
        Pen(A held) { this.held = held; }
        String sound() { return "pen[" + held.sound() + "]"; }
        @Override
        int legs() { return held.legs() + 1; }
    }

    static <A extends Animal> String corral(A a, int n) {
        String here = a.sound() + " " + a.legs();
        if (n == 0) {
            return here;
        }
        return here + " | " + Main.<Pen<A>>corral(new Pen<A>(a), n - 1);
    }

    static class Both<T extends Named & Aged> implements Named, Aged {
        T inner;
        Both(T inner) { this.inner = inner; }
        public String name() { return "both(" + inner.name() + ")"; }
        public int age() { return inner.age() + 10; }
    }

    static <T extends Named & Aged> String pair(T x, int n) {
        String here = x.name() + " aged " + x.age();
        if (n == 0) {
            return here;
        }
        return here + " | " + Main.<Both<T>>pair(new Both<T>(x), n - 1);
    }

    public static void main(String[] args) {
        ArrayList<String> log = new ArrayList<String>();
        System.out.println(Main.<Tag>nest(new Tag("t"), 5, log));
        for (String line : log) {
            System.out.println(line);
        }
        System.out.println(Main.<Dog>corral(new Dog(), 3));
        System.out.println(Main.<Tag>pair(new Tag("p"), 3));
    }
}
