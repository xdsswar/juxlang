public class Main {
    static class Node {
        int value;
        Node next;
        Node(int v) { this.value = v; }
    }

    static void bump(Node n) { n.value = n.value + 100; }
    static void reassign(Node n) { n = new Node(-1); }

    public static void main(String[] args) {
        Node a = new Node(1);
        Node b = a;
        b.value = 5;
        System.out.println(a.value);
        bump(a);
        System.out.println(b.value);
        reassign(a);
        System.out.println(a.value);

        a.next = new Node(2);
        a.next.next = new Node(3);
        System.out.println(a.next.value);
        System.out.println(a.next.next.value);
        Node mid = a.next;
        mid.value = 20;
        System.out.println(a.next.value);
    }
}
