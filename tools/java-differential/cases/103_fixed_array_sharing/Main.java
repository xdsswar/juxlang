// Twin of 91_fixed_array_sharing.jux. Java has one array type, so the Jux
// `int[3]` is an `int[]` here; the point is the same: a callee receives the
// caller's array, not a copy.
public class Main {
    static int sum(int[] xs) {
        int total = 0;
        for (int x : xs) {
            total += x;
        }
        return total;
    }

    static void bump(int[] xs) {
        xs[0] = 100;
    }

    static int[] make() {
        int[] local = {7, 8, 9};
        return local;
    }

    static int size(int[] xs) {
        return xs.length;
    }

    public static void main(String[] args) {
        int[] a = {1, 2, 3};
        System.out.println(sum(a));

        bump(a);
        System.out.println(a[0]);
        System.out.println(a.length);

        int[] b = a;
        b[1] = 50;
        System.out.println(a[1]);

        System.out.println(sum(make()));

        int[] ones = {1, 1, 1, 1};
        System.out.println(size(ones));
    }
}
