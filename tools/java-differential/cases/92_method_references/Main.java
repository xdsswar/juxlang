import java.util.function.BiFunction;
import java.util.function.Function;
import java.util.function.Supplier;

// Twin of 92_method_references.jux: bound method references, overloads picked
// by the target functional type, and overloaded constructor references.
public class Main {
    static class Greeter {
        private String who;

        Greeter(String who) { this.who = who; }

        String greet(String other) { return who + " greets " + other; }

        String greet(String other, int times) { return who + " greets " + other + " x" + times; }

        Function<String, String> greeter() { return this::greet; }
    }

    static class Point {
        int x;
        int y;

        Point() { this(0, 0); }

        Point(int x, int y) {
            this.x = x;
            this.y = y;
        }
    }

    static String apply(Function<String, String> f, String value) { return f.apply(value); }

    public static void main(String[] args) {
        Greeter ann = new Greeter("ann");

        Function<String, String> once = ann::greet;
        BiFunction<String, Integer, String> many = ann::greet;
        System.out.println(once.apply("bob"));
        System.out.println(many.apply("bob", 3));

        System.out.println(apply(ann::greet, "cy"));
        System.out.println(ann.greeter().apply("dee"));

        Supplier<Point> origin = Point::new;
        BiFunction<Integer, Integer, Point> at = Point::new;
        System.out.println(origin.get().x);
        Point p = at.apply(4, 7);
        System.out.println(p.x + p.y);
        System.out.println(at.apply(1, 2).y);
    }
}
