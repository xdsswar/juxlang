import java.util.*;

public class Main {
    static class Node<T> {
        T value;
        List<T> log;
        Node(T value, List<T> log) { this.value = value; this.log = log; }
        List<T> history() { return this.log; }
        int remember(T v) { this.log.add(v); return this.log.size(); }
    }

    static <T> List<T> collect(T x, List<T> into, int n) {
        into.add(x);
        if (n == 0) { return into; }
        List<Node<T>> deeper = new ArrayList<>();
        Node<T> node = new Node<T>(x, into);
        node.remember(x);
        collect(node, deeper, n - 1);
        return into;
    }

    interface Grid<G> { int fill(G rows); }
    interface Slots<A> { int put(A slots); }

    static class Cell implements Grid<List<List<Cell>>>, Slots<Cell[]> {
        int v;
        Cell(int v) { this.v = v; }
        public int fill(List<List<Cell>> rows) {
            List<Cell> row = new ArrayList<>();
            row.add(this);
            rows.add(row);
            if (rows.size() > 1) { rows.get(0).add(this); }
            return rows.size();
        }
        public int put(Cell[] slots) {
            slots[0] = this;
            return slots.length;
        }
    }

    static class Box<T extends Grid<List<List<T>>> & Slots<T[]>> implements Grid<List<List<Box<T>>>>, Slots<Box<T>[]> {
        T inner;
        Box(T inner) { this.inner = inner; }
        public int fill(List<List<Box<T>>> rows) { rows.add(new ArrayList<>()); return 10 + rows.size(); }
        public int put(Box<T>[] slots) { slots[1] = this; return 20 + slots.length; }
    }

    @SuppressWarnings("unchecked")
    static <T extends Grid<List<List<T>>> & Slots<T[]>> int go(T x, T y, int n) {
        if (n == 0) {
            List<List<T>> rows = new ArrayList<>();
            int a = x.fill(rows);
            int b = x.fill(rows);
            List<T> firstRow = rows.get(0);
            int c = x.fill(rows);
            T[] slots = (T[]) java.lang.reflect.Array.newInstance(y.getClass(), 2);
            slots[0] = y;
            slots[1] = y;
            int d = x.put(slots);
            boolean same = slots[0] == x;
            return a * 100000 + b * 10000 + c * 1000 + firstRow.size() * 100 + d * 10 + (same ? 1 : 0);
        }
        return go(new Box<T>(x), new Box<T>(y), n - 1);
    }

    public static void main(String[] args) {
        List<String> mine = new ArrayList<>();
        List<String> back = collect("a", mine, 2);
        System.out.println(mine.size() + " " + back.size());
        back.add("z");
        System.out.println(mine.size());

        List<Integer> log = new ArrayList<>();
        Node<Integer> n = new Node<Integer>(5, log);
        n.remember(6);
        System.out.println(log.size());
        List<Integer> h = n.history();
        h.add(7);
        System.out.println(n.log.size() + " " + log.size());
        n.log = new ArrayList<>();
        n.log.add(1);
        System.out.println(n.remember(2) + " " + log.size());

        System.out.println(go(new Cell(1), new Cell(2), 0));
        System.out.println(go(new Cell(1), new Cell(2), 2));
    }
}
