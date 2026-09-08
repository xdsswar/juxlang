import java.util.ArrayList;

public class Main {
    static int firstPlusOne(int[] xs) {
        return xs[0] + 1;
    }

    public static void main(String[] args) {
        int[] a = new int[5];
        a[0] = 4; a[1] = 3; a[2] = 2; a[3] = 1; a[4] = 0;

        int t = a[0];
        a[0] = a[1];
        a[1] = t;
        System.out.println(a[0]);
        System.out.println(a[1]);

        a[a[4]] = 77;
        System.out.println(a[0]);

        a[2] = firstPlusOne(a);
        System.out.println(a[2]);

        a[3] += a[2];
        System.out.println(a[3]);

        ArrayList<ArrayList<Integer>> rows = new ArrayList<>();
        ArrayList<Integer> r0 = new ArrayList<>();
        r0.add(10);
        r0.add(20);
        rows.add(r0);
        rows.get(0).set(1, rows.get(0).get(0));
        System.out.println(rows.get(0).get(0));
        System.out.println(rows.get(0).get(1));

        int i = 4;
        a[i] = 9;
        System.out.println(a[4]);
    }
}
