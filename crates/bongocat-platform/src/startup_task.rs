pub(super) fn definition(user: &str, executable: &str, elevated: bool) -> String {
    let user = escape(user);
    let executable = escape(executable);
    let level = if elevated {
        "HighestAvailable"
    } else {
        "LeastPrivilege"
    };
    format!(
        r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
<Principals><Principal id="User"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>{level}</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Enabled>true</Enabled></Settings>
<Actions Context="User"><Exec><Command>{executable}</Command><Arguments>--run-seconds 0</Arguments></Exec></Actions>
</Task>"#
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_login_task_keeps_privilege_and_lifetime_explicit() {
        let xml = definition(r"DOMAIN\user", r"C:\Program Files\BongoCat.exe", true);
        assert_eq!(xml.matches(r"<UserId>DOMAIN\user</UserId>").count(), 2);
        for expected in [
            "InteractiveToken",
            "HighestAvailable",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<Arguments>--run-seconds 0</Arguments>",
        ] {
            assert!(xml.contains(expected));
        }
        assert!(definition("user", "app.exe", false).contains("LeastPrivilege"));
    }

    #[test]
    fn xml_special_characters_and_unicode_are_literal_data() {
        let xml = definition("user<&", "C:\\猫 & <cat>\"'.exe", true);
        assert!(xml.contains("user&lt;&amp;"));
        assert!(xml.contains("猫 &amp; &lt;cat&gt;&quot;&apos;.exe"));
    }
}
