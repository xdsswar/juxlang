public class Main {
    static class Ids {
        private static int next = 0;

        static int take() {
            next = next + 1;
            return next;
        }

        static int peek() { return next; }
        static void reset() { next = 0; }
    }

    static class Node {
        private int id;
        private String tag;
        Node(String tag) {
            this.id = Ids.take();
            this.tag = tag;
        }
        String describe() { return this.id + ":" + this.tag; }
    }

    public static void main(String[] args) {
        System.out.println(Ids.peek());
        Node a = new Node("a");
        Node b = new Node("b");
        System.out.println(a.describe());
        System.out.println(b.describe());
        System.out.println(Ids.peek());
        System.out.println(Ids.take());
        Ids.reset();
        System.out.println(Ids.peek());
        Node c = new Node("c");
        System.out.println(c.describe());
        System.out.println(a.describe());
    }
}
