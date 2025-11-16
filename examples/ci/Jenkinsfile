pipeline {
    agent any
    
    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }
        
        stage('Install Feluda') {
            steps {
                sh 'cargo install feluda'
            }
        }
        
        stage('Check Licenses') {
            steps {
                sh 'feluda --ci-format jenkins --output-file feluda-results.xml'
            }
            post {
                always {
                    junit 'feluda-results.xml'
                }
            }
        }
    }
}
