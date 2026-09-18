class Animal {
    public String name() { return "animal"; }
}

class Dog extends Animal {
    public String bark() { return "woof"; }
}

class Node {
    int value;
    Node next;
    Node(int value) { this.value = value; }
}

public class Main {
    static String require(String s) {
        if (s == null) throw new IllegalArgumentException("missing value");
        return s;
    }

    static String shout(String s) {
        assert s != null;
        return s.toUpperCase() + "!";
    }

    static int total(Node head) {
        Node cur = head;
        int sum = 0;
        while (cur != null) {
            sum += cur.value;
            cur = cur.next;
        }
        return sum;
    }

    static int count(Node head) {
        int n = 0;
        Node cur = head;
        while (cur != null) {
            n += 1;
            cur = cur.next;
        }
        return n;
    }

    public static void main(String[] args) {
        Animal pet = new Dog();
        if (pet instanceof Dog d) System.out.println(d.bark());
        if (pet instanceof Dog d) {
            System.out.println(d.name() + " says " + d.bark());
        }

        System.out.println(require("ok"));
        try {
            System.out.println(require(null));
        } catch (IllegalArgumentException e) {
            System.out.println("caught: " + e.getMessage());
        }

        System.out.println(shout("hey"));

        Node list = new Node(1);
        list.next = new Node(2);
        list.next.next = new Node(3);
        System.out.println(total(list));
        System.out.println(count(list));
        System.out.println(total(null));
    }
}
