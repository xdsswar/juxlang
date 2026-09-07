import java.util.ArrayList;
import java.util.Collections;

public class Main {
    static void addAll(ArrayList<Integer> dst, int n) {
        for (int i = 0; i < n; i++) {
            dst.add(i);
        }
    }

    public static void main(String[] args) {
        ArrayList<Integer> xs = new ArrayList<>();
        addAll(xs, 5);
        System.out.println(xs.size());
        System.out.println(xs.get(0));
        System.out.println(xs.get(4));

        ArrayList<Integer> alias = xs;
        alias.add(99);
        System.out.println(xs.size());
        System.out.println(xs.get(5));

        int sum = 0;
        for (int v : xs) {
            sum = sum + v;
        }
        System.out.println(sum);

        int last = xs.remove(xs.size() - 1);
        System.out.println(last);
        System.out.println(xs.size());

        ArrayList<String> names = new ArrayList<>();
        names.add("b");
        names.add("a");
        names.add("c");
        Collections.sort(names);
        System.out.println(names.get(0));
        System.out.println(names.get(2));
        System.out.println(names.size());
    }
}
