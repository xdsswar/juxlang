import java.util.function.Function;
import java.util.function.IntUnaryOperator;

public class Main {
    static int applyTo(int x, IntUnaryOperator f) {
        return f.applyAsInt(x);
    }

    static String twice(String s, Function<String, String> f) {
        return f.apply(f.apply(s));
    }

    static int sumWith(java.util.List<Integer> xs, IntUnaryOperator f) {
        int total = 0;
        for (int x : xs) {
            total = total + f.applyAsInt(x);
        }
        return total;
    }

    public static void main(String[] args) {
        System.out.println(applyTo(5, n -> n * 2));
        System.out.println(applyTo(5, n -> n + 100));
        System.out.println(twice("a", s -> s + "!"));

        int offset = 10;
        System.out.println(applyTo(1, n -> n + offset));

        java.util.List<Integer> xs = new java.util.ArrayList<>();
        xs.add(1);
        xs.add(2);
        xs.add(3);
        System.out.println(sumWith(xs, n -> n * n));
        System.out.println(sumWith(xs, n -> 1));

        int running = 0;
        for (int x : xs) {
            running = running + x;
        }
        System.out.println(running);
    }
}
