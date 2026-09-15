import java.util.HashSet;

public class Main {
    interface Shape { double area(); }

    static class Node implements Shape {
        int id;
        Node(int id) { this.id = id; }
        public double area() { return 0.0; }
    }

    static class Leaf extends Node {
        Leaf(int id) { super(id); }
    }

    static class Money {
        int cents;
        Money(int c) { cents = c; }
        @Override public boolean equals(Object o) { return o instanceof Money && cents == ((Money) o).cents; }
        @Override public int hashCode() { return cents; }
        @Override public String toString() { return "$" + cents; }
    }

    static class Coin extends Money {
        Coin(int c) { super(c); }
    }

    static class Note extends Money {
        Note(int c) { super(c); }
        @Override public boolean equals(Object o) { return false; }
        @Override public int hashCode() { return 7; }
        @Override public String toString() { return "note " + cents; }
    }

    public static void main(String[] args) {
        Leaf leaf = new Leaf(1);
        Node node = leaf;
        Shape shape = leaf;
        Node other = new Leaf(1);
        System.out.println(node.equals(leaf));
        System.out.println(shape == node);
        System.out.println(node.equals(other));
        System.out.println(!node.equals(other));
        HashSet<Node> nodes = new HashSet<>();
        nodes.add(node);
        nodes.add(leaf);
        nodes.add(other);
        System.out.println(nodes.size());

        Coin a = new Coin(5);
        Coin b = new Coin(5);
        Money m = a;
        Money n = new Note(5);
        System.out.println(a.equals(b));
        System.out.println(a == b);
        System.out.println(m.equals(b));
        System.out.println(n.equals(m));
        System.out.println(m.equals(n));
        System.out.println(a);
        System.out.println(n);
        HashSet<Money> wallet = new HashSet<>();
        wallet.add(m);
        wallet.add(b);
        wallet.add(n);
        System.out.println(wallet.size());
    }
}
