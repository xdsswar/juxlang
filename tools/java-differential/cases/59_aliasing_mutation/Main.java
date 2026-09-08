public class Main {
    static class Counter {
        int n;
        Counter(int n) { this.n = n; }
        void bump() { this.n = this.n + 1; }
    }

    static void bumpTwice(Counter c) {
        c.bump();
        c.bump();
    }

    static void addAll(java.util.List<Integer> target, int howMany) {
        for (int i = 0; i < howMany; i++) {
            target.add(i);
        }
    }

    static void replaceFirst(java.util.List<String> names, String with) {
        names.set(0, with);
    }

    public static void main(String[] args) {
        Counter a = new Counter(0);
        Counter b = a;
        a.bump();
        System.out.println(a.n);
        System.out.println(b.n);
        bumpTwice(b);
        System.out.println(a.n);

        java.util.List<Integer> xs = new java.util.ArrayList<>();
        addAll(xs, 3);
        System.out.println(xs.size());
        System.out.println(xs.get(2));

        java.util.List<String> names = new java.util.ArrayList<>();
        names.add("one");
        names.add("two");
        java.util.List<String> alias = names;
        replaceFirst(alias, "ONE");
        System.out.println(names.get(0));
        System.out.println(alias.get(1));

        int[] arr = new int[2];
        arr[0] = 5;
        int[] arrAlias = arr;
        arrAlias[0] = 6;
        System.out.println(arr[0]);
    }
}
