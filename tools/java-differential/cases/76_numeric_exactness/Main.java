import java.util.ArrayList;

public class Main {
    record Reading(double value, float gain) {
        public String toString() { return "Reading(value: " + value + ", gain: " + gain + ")"; }
    }

    static String level(double db) { return "Level(db: " + db + ")"; }

    static class Holder<T> {
        private T item;
        Holder(T item) { this.item = item; }
        String show() { return "Holder(" + item + ")"; }
    }

    static final double LIMIT = 6.0;

    static double twice(float x) { return x * 2; }

    static int lastIndex(ArrayList<String> names) { return names.size() - 1; }

    public static void main(String[] args) {
        System.out.println(1.0);
        System.out.println(100.0 / 3.0);
        System.out.println(1e7);
        System.out.println(0.001);
        System.out.println(1e-4);
        System.out.println(1e21);
        System.out.println(0.1 + 0.2);
        System.out.println(1.0 / 0.0);
        System.out.println(-1.0 / 0.0);
        System.out.println(0.0 / 0.0);
        System.out.println(-0.0);
        System.out.println(Double.MIN_VALUE);
        System.out.println(Float.MIN_VALUE);
        System.out.println(0.1f);
        System.out.println(new Reading(2.0, 0.5f));
        System.out.println(level(50.0));
        System.out.println(new Holder<Double>(4.0).show());
        System.out.println(LIMIT);
        System.out.println("interpolated " + 3.0 + " and concatenated " + 7.0);

        int k = 4;
        System.out.println(k / 2 * 1.0);
        int a = 3;
        int b = 4;
        System.out.println(a * b * 0.5);
        int small = 2147483600;
        System.out.println(small + 7);
        long wide = small + 1000000000L;
        System.out.println(wide);
        boolean flag = k > 2;
        System.out.println(flag ? 1 : 2.5);
        System.out.println(twice(1.25f));

        float f = 1.5f;
        double d = f;
        short s = 7;
        int fromShort = s;
        double[] halves = {1, 2, 3};
        d += 1;
        d += k;
        d++;
        System.out.println(d);
        System.out.println(fromShort);
        System.out.println(halves[2]);

        long acc = 1;
        for (int i = 0; i < 40; i++) {
            acc = acc * 2;
        }
        System.out.println(acc);

        System.out.println((int) 1e30);
        System.out.println((long) (0.0 / 0.0));
        System.out.println((float) 1e40);
        System.out.println((int) -2.9);

        long one = 1L;
        int by = 65;
        System.out.println(one << by);

        System.out.println(-7 / 2);
        System.out.println(-7 % 3);

        ArrayList<String> names = new ArrayList<>();
        System.out.println(lastIndex(names));
        int before = -1;
        System.out.println(before < names.size());
    }
}
