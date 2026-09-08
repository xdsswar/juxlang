public class Main {
    static class Cell<T> {
        private T v;
        Cell(T v) { this.v = v; }
        T get() { return this.v; }
        void set(T v) { this.v = v; }
        String show() { return "Cell(" + this.v + ")"; }
    }

    static int sumRows(java.util.List<java.util.List<Integer>> rows) {
        int t = 0;
        for (java.util.List<Integer> row : rows) {
            for (int x : row) {
                t = t + x;
            }
        }
        return t;
    }

    public static void main(String[] args) {
        java.util.List<java.util.List<Integer>> rows = new java.util.ArrayList<>();
        java.util.List<Integer> r0 = new java.util.ArrayList<>();
        r0.add(1);
        r0.add(2);
        java.util.List<Integer> r1 = new java.util.ArrayList<>();
        r1.add(10);
        r1.add(20);
        rows.add(r0);
        rows.add(r1);
        System.out.println(sumRows(rows));
        System.out.println(rows.size());
        System.out.println(rows.get(0).size());
        System.out.println(rows.get(1).get(0));

        r0.add(3);
        System.out.println(rows.get(0).size());
        System.out.println(sumRows(rows));

        Cell<Cell<Cell<Integer>>> deep = new Cell<>(new Cell<>(new Cell<>(5)));
        System.out.println(deep.get().get().get());
        System.out.println(deep.get().get().show());
        deep.get().get().set(9);
        System.out.println(deep.get().get().get());

        java.util.List<Cell<String>> cells = new java.util.ArrayList<>();
        cells.add(new Cell<>("a"));
        cells.add(new Cell<>("b"));
        System.out.println(cells.get(0).show());
        System.out.println(cells.get(1).get());
        cells.get(0).set("z");
        System.out.println(cells.get(0).get());
    }
}
