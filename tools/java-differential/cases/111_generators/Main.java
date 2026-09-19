import java.util.ArrayDeque;
import java.util.Deque;
import java.util.Iterator;

// Twin of 111_generators.jux. Each Jux generator is written out here as the
// Iterator class it replaces. Jux's next() answers null at the end, so these
// do too, rather than throwing NoSuchElementException.
public class Main {
    static class Squares implements Iterator<Integer> {
        private int i = 1;
        private final int n;

        Squares(int n) { this.n = n; }

        public boolean hasNext() { return i <= n; }

        public Integer next() {
            if (i > n) return null;
            int v = i * i;
            i++;
            return v;
        }
    }

    static class Countdown implements Iterator<Integer> {
        private int n;

        Countdown(int from) { this.n = from; }

        public boolean hasNext() { return n >= 0; }

        public Integer next() {
            if (n < 0) return null;
            return n--;
        }
    }

    static class Concat implements Iterator<Integer> {
        private final Iterator<Integer> a;
        private final Iterator<Integer> b;

        Concat(Iterator<Integer> a, Iterator<Integer> b) {
            this.a = a;
            this.b = b;
        }

        public boolean hasNext() { return a.hasNext() || b.hasNext(); }

        public Integer next() {
            if (a.hasNext()) return a.next();
            if (b.hasNext()) return b.next();
            return null;
        }
    }

    static class Fibonacci implements Iterator<Long> {
        private long a = 0;
        private long b = 1;

        public boolean hasNext() { return true; }

        public Long next() {
            long v = a;
            long next = a + b;
            a = b;
            b = next;
            return v;
        }
    }

    static class Node {
        int value;
        Node left;
        Node right;

        Node(int value, Node left, Node right) {
            this.value = value;
            this.left = left;
            this.right = right;
        }
    }

    static class InOrder implements Iterator<Integer> {
        private final Deque<Node> stack = new ArrayDeque<>();

        InOrder(Node root) { pushLeft(root); }

        private void pushLeft(Node n) {
            while (n != null) {
                stack.push(n);
                n = n.left;
            }
        }

        public boolean hasNext() { return !stack.isEmpty(); }

        public Integer next() {
            if (stack.isEmpty()) return null;
            Node n = stack.pop();
            pushLeft(n.right);
            return n.value;
        }
    }

    static class Account {
        private int balance;

        Account(int balance) { this.balance = balance; }

        Iterator<Integer> statements(int months) {
            return new Iterator<Integer>() {
                private int m = 0;

                public boolean hasNext() { return m < months; }

                public Integer next() {
                    if (m >= months) return null;
                    m++;
                    balance = balance + 10;
                    return balance;
                }
            };
        }

        void withdraw(int amount) { balance = balance - amount; }
    }

    public static void main(String[] args) {
        Iterator<Integer> sq = new Squares(5);
        while (sq.hasNext()) {
            System.out.println("square " + sq.next());
        }

        Iterator<Integer> cat = new Concat(new Countdown(2), new Squares(2));
        while (cat.hasNext()) {
            System.out.println("concat " + cat.next());
        }

        Fibonacci fib = new Fibonacci();
        for (int i = 0; i < 8; i++) {
            System.out.println("fib " + fib.next());
        }

        Node root = new Node(5,
            new Node(3, new Node(1, null, null), new Node(4, null, null)),
            new Node(8, null, new Node(9, null, null)));
        Iterator<Integer> tree = new InOrder(root);
        while (tree.hasNext()) {
            System.out.println("tree " + tree.next());
        }

        Account acct = new Account(100);
        Iterator<Integer> st = acct.statements(3);
        while (st.hasNext()) {
            System.out.println("balance " + st.next());
            acct.withdraw(25);
        }

        Countdown it = new Countdown(6);
        while (it.hasNext()) {
            int n = it.next();
            if (n == 4) break;
        }
        System.out.println("next " + it.next());
        System.out.println("next " + it.next());

        Countdown done = new Countdown(0);
        System.out.println("first " + done.next());
        System.out.println("after " + done.next());
        System.out.println("again " + done.next());
    }
}
