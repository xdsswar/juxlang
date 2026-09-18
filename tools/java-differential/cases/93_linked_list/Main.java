public class Main {
    static class Node {
        int value;
        Node next = null;
        Node(int value) { this.value = value; }
    }

    static class IntList {
        private Node head = null;
        private int count = 0;

        void pushFront(int v) {
            Node n = new Node(v);
            n.next = head;
            head = n;
            count = count + 1;
        }

        void pushBack(int v) {
            Node n = new Node(v);
            if (head == null) {
                head = n;
            } else {
                Node cur = head;
                while (cur.next != null) {
                    cur = cur.next;
                }
                cur.next = n;
            }
            count = count + 1;
        }

        boolean remove(int v) {
            if (head == null) { return false; }
            Node first = head;
            if (first.value == v) {
                head = first.next;
                count = count - 1;
                return true;
            }
            Node prev = first;
            while (prev.next != null) {
                Node cur = prev.next;
                if (cur.value == v) {
                    prev.next = cur.next;
                    count = count - 1;
                    return true;
                }
                prev = cur;
            }
            return false;
        }

        void reverse() {
            Node prev = null;
            Node cur = head;
            while (cur != null) {
                Node n = cur;
                Node after = n.next;
                n.next = prev;
                prev = n;
                cur = after;
            }
            head = prev;
        }

        int size() { return count; }

        int lengthOf(Node n) {
            if (n == null) { return 0; }
            return 1 + lengthOf(n.next);
        }

        int recursiveLength() { return lengthOf(head); }

        String show() {
            String out = "[";
            Node cur = head;
            boolean first = true;
            while (cur != null) {
                Node n = cur;
                if (!first) { out = out + ", "; }
                out = out + n.value;
                first = false;
                cur = n.next;
            }
            return out + "]";
        }
    }

    public static void main(String[] args) {
        IntList list = new IntList();
        System.out.println(list.show());
        list.pushBack(2);
        list.pushBack(3);
        list.pushFront(1);
        list.pushBack(4);
        System.out.println(list.show() + " size " + list.size());
        System.out.println(list.remove(3));
        System.out.println(list.remove(9));
        System.out.println(list.remove(1));
        System.out.println(list.show() + " size " + list.size());
        list.pushFront(7);
        list.pushBack(8);
        list.reverse();
        System.out.println(list.show());
        System.out.println(list.recursiveLength() == list.size());
    }
}
