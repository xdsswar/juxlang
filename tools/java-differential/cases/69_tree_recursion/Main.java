public class Main {
    static class Node {
        int value;
        Node left;
        Node right;
        Node(int value) {
            this.value = value;
            this.left = null;
            this.right = null;
        }
    }

    static Node insert(Node root, int v) {
        if (root == null) {
            return new Node(v);
        }
        if (v < root.value) {
            root.left = insert(root.left, v);
        } else {
            root.right = insert(root.right, v);
        }
        return root;
    }

    static String inorder(Node n) {
        if (n == null) {
            return "";
        }
        return inorder(n.left) + n.value + " " + inorder(n.right);
    }

    static int depth(Node n) {
        if (n == null) {
            return 0;
        }
        int l = depth(n.left);
        int r = depth(n.right);
        if (l > r) {
            return l + 1;
        }
        return r + 1;
    }

    static int count(Node n) {
        if (n == null) {
            return 0;
        }
        return 1 + count(n.left) + count(n.right);
    }

    public static void main(String[] args) {
        Node root = null;
        int[] values = new int[7];
        values[0] = 5;
        values[1] = 3;
        values[2] = 8;
        values[3] = 1;
        values[4] = 4;
        values[5] = 7;
        values[6] = 9;
        for (int i = 0; i < 7; i++) {
            root = insert(root, values[i]);
        }
        System.out.println(inorder(root));
        System.out.println(depth(root));
        System.out.println(count(root));
        System.out.println(root.value);
        System.out.println(root.left.value);
        System.out.println(root.right.value);
    }
}
